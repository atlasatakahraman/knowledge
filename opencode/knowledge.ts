// knowledge-skeleton's opencode plugin (vault audit S7.5). Written to ~/.config/opencode/plugins/
// by `knowledge setup --harness opencode`, which replaces any edit made here.
//
// It only spawns the command: each opencode hook becomes one `knowledge hook <verb> --harness
// opencode` call, a canonical JSON payload on stdin and one JSON reply on stdout, and the reply is
// acted on here. The binary decides everything else. The pin is the version this file was written
// for: a binary of another version stands down and says so once, at session start.
//
// The hooks, as read from opencode 2.2.1 (the S7.0 probe and the binary itself):
//   event session.created             -> session   (not for a subagent's session: it has a parentID)
//   session hook "prompt"             -> prompt
//   session hook "context"            -> nothing spawned: it pushes the kept context into `system`
//   tool hook "execute.before"        -> pre-tool  (shell and the edit tools; a deny throws)
//   tool hook "execute.after"         -> post-edit or post-bash, by tool name
//   event session.compaction.started  -> capture   (pre-compact)
//   event session.execution.succeeded -> capture   (stop; opencode 2.2.1 has no session.idle)
//   event session.deleted             -> capture   (session end)
import { spawn } from "node:child_process";

const PIN = "0.12.0";
const EDITS = new Set(["edit", "write", "patch"]);

type Reply = { context?: string; decision?: string; reason?: string };

// One hook call. Fails silent: a missing binary, a timeout or bad output is an empty reply, so a
// broken install never blocks opencode (the guard allows, as the binary's own guard does).
function knowledge(verb: string, payload: object, ms: number): Promise<Reply> {
  return new Promise((done) => {
    let out = "";
    let child: ReturnType<typeof spawn>;
    try {
      child = spawn("knowledge", ["hook", verb, "--harness", "opencode", "--pin", PIN], {
        stdio: ["pipe", "pipe", "ignore"],
        windowsHide: true,
      });
    } catch {
      return done({});
    }
    const timer = setTimeout(() => child.kill(), ms);
    child.stdout?.on("data", (b) => (out += b));
    child.on("error", () => {
      clearTimeout(timer);
      done({});
    });
    child.on("close", () => {
      clearTimeout(timer);
      try {
        done(out.trim() ? JSON.parse(out) : {});
      } catch {
        done({});
      }
    });
    child.stdin?.on("error", () => {});
    child.stdin?.end(JSON.stringify(payload));
  });
}

export default {
  id: "knowledge-skeleton",
  async setup(ctx: any) {
    // The flush's own `opencode run --standalone` inherits this from knowledge: no hooks inside it.
    if (process.env.KNOWLEDGE_INVOKED_BY) return;

    const cwd = new Map<string, string>(); // session -> its directory
    const children = new Set<string>(); // subagent sessions
    const start = new Map<string, Promise<string | undefined>>(); // session -> its start context
    const turn = new Map<string, string[]>(); // session -> this turn's context

    const send = (verb: string, name: string, sid: string, more: object = {}, ms = 10_000) =>
      knowledge(
        verb,
        {
          hook_event_name: name,
          session_id: sid,
          cwd: cwd.get(sid) ?? ctx.location?.directory ?? process.cwd(),
          ...more,
        },
        ms,
      );
    const begin = (sid: string, name: string) =>
      start.set(sid, send("session", name, sid).then((r) => r.context));

    const controller = new AbortController();
    void (async () => {
      try {
        for await (const e of ctx.event.subscribe({ signal: controller.signal })) {
          const d = e?.data ?? {};
          const sid = d.sessionID;
          if (typeof sid !== "string") continue;
          switch (e.type) {
            case "session.created":
              if (typeof d.location?.directory === "string") cwd.set(sid, d.location.directory);
              if (d.parentID) children.add(sid);
              else begin(sid, e.type);
              break;
            case "session.compaction.started":
            case "session.execution.succeeded":
              void send("capture", e.type, sid);
              break;
            case "session.deleted":
              await send("capture", e.type, sid);
              for (const m of [cwd, start, turn]) m.delete(sid);
              children.delete(sid);
              break;
          }
        }
      } catch {}
    })();

    await ctx.session.hook("prompt", async (e: any) => {
      const sid = e.sessionID;
      // A session opened before this plugin loaded (opencode's service restarted) still starts once.
      if (!start.has(sid) && !children.has(sid)) begin(sid, "prompt");
      const r = await send("prompt", "prompt", sid, { prompt: e.prompt?.text ?? "" });
      turn.set(sid, r.context ? [r.context] : []);
    });

    // `system` is rebuilt for every model call, so the kept context goes in every time.
    await ctx.session.hook("context", async (e: any) => {
      const texts = [await start.get(e.sessionID), ...(turn.get(e.sessionID) ?? [])];
      for (const text of texts) if (text) e.system.push({ type: "text", text });
    });

    await ctx.tool.hook("execute.before", async (e: any) => {
      if (e.tool !== "shell" && !EDITS.has(e.tool)) return;
      const r = await send(
        "pre-tool",
        "execute.before",
        e.sessionID,
        { tool_name: e.tool, tool_input: e.input },
        5_000,
      );
      if (r.decision === "deny") throw new Error(r.reason || "denied by the knowledge guard");
    });

    await ctx.tool.hook("execute.after", async (e: any) => {
      if (e.status !== "completed") return;
      if (EDITS.has(e.tool)) {
        const r = await send("post-edit", "execute.after", e.sessionID, {
          tool_name: e.tool,
          tool_input: e.input,
        });
        // A vault note's check reaches the model at its next call, inside this turn.
        if (r.context) turn.set(e.sessionID, [...(turn.get(e.sessionID) ?? []), r.context]);
      } else if (e.tool === "shell") {
        void send("post-bash", "execute.after", e.sessionID, {
          tool_name: e.tool,
          tool_input: e.input,
        }, 15_000);
      }
    });

    return () => controller.abort();
  },
};
