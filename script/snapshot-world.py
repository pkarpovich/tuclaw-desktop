#!/usr/bin/env python3
import json
import os
import sqlite3
import sys
from datetime import datetime
from pathlib import Path

SUPPORT = Path.home() / "Library" / "Application Support" / "tuclaw-desktop"


def surface_name(row, topics):
    if row["name"]:
        return row["name"]
    if row["topic_id"] == 0:
        return "General"
    named = topics.get((row["chat_id"], row["topic_id"]))
    if named:
        return named
    return f"topic-{row['topic_id']}"


def surfaces(db):
    topics = {}
    for row in db.execute("select chat_id, topic_id, display_name from topics"):
        topics[(row["chat_id"], row["topic_id"])] = row["display_name"]
    orders = {}
    for row in db.execute("select context_name, sort_order from context_settings"):
        orders[row["context_name"]] = row["sort_order"]
    latest = {}
    for row in db.execute(
        "select surface_id, max(created_at) as at from messages where kind <> 'prompt' group by surface_id"
    ):
        latest[row["surface_id"]] = row["at"]
    result = []
    for row in db.execute("select * from surfaces order by id"):
        wiring = []
        lead = None
        for wired in db.execute(
            "select agent_id, engage_mode, ignored_message_policy from surface_agents where surface_id = ? order by priority desc, id",
            (row["id"],),
        ):
            role = "lead" if wired["engage_mode"] == "default" else "mention"
            if role == "lead":
                lead = wired["agent_id"]
            wiring.append(
                {
                    "agent_id": wired["agent_id"],
                    "role": role,
                    "listens": wired["ignored_message_policy"] == "accumulate",
                }
            )
        bindings = []
        for bound in db.execute(
            "select channel, external_id, mirror from surface_bindings where surface_id = ?",
            (row["id"],),
        ):
            bindings.append(dict(bound))
        context = f"chat-{row['chat_id']}-topic-{row['topic_id']}"
        result.append(
            {
                "id": row["id"],
                "kind": row["kind"],
                "name": surface_name(row, topics),
                "sort_order": orders.get(context, 0),
                "last_message_at": latest.get(row["id"]),
                "lead_agent_id": lead,
                "agents": wiring,
                "bindings": bindings,
            }
        )
    result.sort(key=lambda surface: (surface["sort_order"], surface["id"]))
    return result


def agents(db):
    result = []
    for row in db.execute(
        "select a.*, s.surface_id as home, s.model_override as model from agents a left join sessions s on s.agent_id = a.id order by a.id"
    ):
        ident = row["delegate_name"] or row["bot_username"] or row["name"]
        result.append(
            {
                "id": row["id"],
                "name": row["name"],
                "ident": ident,
                "description": row["description"] or "",
                "bot_username": row["bot_username"],
                "model": row["model"] or "",
                "state": "idle",
                "live_run": None,
                "home_surface_id": row["home"],
            }
        )
    return result


def runs(db):
    result = []
    summaries = {}
    for row in db.execute("select * from runs order by started_at"):
        steps = []
        tools = 0
        for step in db.execute("select * from run_steps where run_id = ? order by seq", (row["id"],)):
            entry = {"seq": step["seq"], "kind": step["kind"], "started_at": step["started_at"]}
            for key in ("tool_use_id", "name", "output", "status", "finished_at"):
                if step[key] is not None:
                    entry[key] = step[key]
            if step["input"] is not None:
                try:
                    entry["input"] = json.loads(step["input"])
                except json.JSONDecodeError:
                    entry["input"] = {"truncated": step["input"]}
            if step["kind"] == "tool":
                tools += 1
            steps.append(entry)
        usage = None
        if row["input_tokens"] is not None:
            usage = {
                "input_tokens": row["input_tokens"] or 0,
                "output_tokens": row["output_tokens"] or 0,
                "cache_read_tokens": row["cache_read_tokens"] or 0,
                "cache_creation_tokens": row["cache_creation_tokens"] or 0,
            }
        context = None
        if row["context_tokens"] is not None:
            context = {
                "tokens": row["context_tokens"],
                "max_tokens": row["context_max_tokens"] or 0,
                "model": row["model"] or "",
            }
        result.append(
            {
                "run": {
                    "id": row["id"],
                    "agent_id": row["agent_id"],
                    "surface_id": row["surface_id"],
                    "origin": row["origin"],
                    "kind": row["kind"],
                    "status": row["status"],
                    "terminal_reason": row["terminal_reason"],
                    "error": row["error"],
                    "started_at": row["started_at"],
                    "finished_at": row["finished_at"],
                    "usage": usage,
                    "context": context,
                },
                "steps": steps,
            }
        )
        summaries[row["id"]] = {
            "status": row["status"],
            "step_count": len(steps),
            "tool_count": tools,
            "started_at": row["started_at"],
            "finished_at": row["finished_at"],
        }
    return result, summaries


def parse_time(text):
    return datetime.fromisoformat(text.replace("Z", "+00:00"))


def millis(start, end):
    if not start or not end:
        return 0
    return max(0, int((parse_time(end) - parse_time(start)).total_seconds() * 1000))


def messages(db, summaries):
    result = []
    for row in db.execute(
        "select * from messages where kind <> 'prompt' and surface_id is not null order by id"
    ):
        summary = None
        run = summaries.get(row["run_id"]) if row["run_id"] else None
        if run:
            summary = {
                "status": run["status"],
                "step_count": run["step_count"],
                "tool_count": run["tool_count"],
                "duration_ms": millis(run["started_at"], run["finished_at"]),
            }
        author = {"kind": row["author_kind"] or "system"}
        if row["author_agent_id"] is not None:
            author["agent_id"] = row["author_agent_id"]
        result.append(
            {
                "id": row["id"],
                "surface_id": row["surface_id"],
                "kind": row["kind"],
                "author": author,
                "addressed_agent_id": row["addressed_agent_id"],
                "reply_to_message_id": None,
                "text": row["text"],
                "run_id": row["run_id"],
                "origin": "user" if row["kind"] == "user" else "agent",
                "channel": "telegram" if row["kind"] == "user" else None,
                "client_message_id": None,
                "created_at": row["created_at"],
                "run_summary": summary,
            }
        )
    return result


def attachments(directory):
    manifest = directory / "attachments.json"
    if not manifest.is_file():
        return {}, []
    by_message = {}
    media = []
    for entry in json.loads(manifest.read_text()):
        files = sorted(directory.glob(f"{entry['id']}.*"))
        if not files:
            continue
        by_message.setdefault(entry["message_id"], []).append(
            {
                "id": entry["id"],
                "kind": entry["kind"],
                "mime": entry["mime"],
                "size_bytes": entry["size_bytes"],
                "duration_ms": entry.get("duration_ms"),
            }
        )
        media.append({"id": entry["id"], "path": str(files[0])})
    return by_message, media


def main():
    source = Path(sys.argv[1]) if len(sys.argv) > 1 else SUPPORT / "snapshot.db"
    target = Path(sys.argv[2]) if len(sys.argv) > 2 else SUPPORT / "world.json"
    db = sqlite3.connect(f"file:{source}?mode=ro", uri=True)
    db.row_factory = sqlite3.Row
    run_list, summaries = runs(db)
    attached, media = attachments(SUPPORT / "attachments")
    message_list = messages(db, summaries)
    for message in message_list:
        message["attachments"] = attached.get(message["id"], [])
    world = {
        "surfaces": surfaces(db),
        "agents": agents(db),
        "messages": message_list,
        "runs": run_list,
        "media": media,
    }
    descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w") as handle:
        json.dump(world, handle, ensure_ascii=False)
    print(
        f"{target}: {len(world['surfaces'])} surfaces, {len(world['agents'])} agents, "
        f"{len(world['messages'])} messages, {len(world['runs'])} runs, {len(media)} attachments"
    )


if __name__ == "__main__":
    main()
