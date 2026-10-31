"""Exercise the real Rust bridge and shortlist persistence in an isolated profile.

Never connects to or reads/writes FM. Does not open windows.
Usage: python tools/test-shortlists-offline.py <engine.exe> <report.json>
"""
import copy
import json
import os
from pathlib import Path
import queue
import struct
import subprocess
import sys
import tempfile
import threading


def run(engine: Path, report: Path):
    report.parent.mkdir(parents=True, exist_ok=True)
    profile = Path(tempfile.mkdtemp(prefix="shortlist-test-", dir=report.parent)).resolve()
    env = dict(os.environ, LOCALAPPDATA=str(profile))
    proc = subprocess.Popen(
        [str(engine.resolve()), "--bridge"], stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8",
        env=env, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    replies = queue.Queue()
    threading.Thread(target=lambda: [replies.put(line) for line in proc.stdout], daemon=True).start()
    tests = []
    sequence = 0

    def check(condition, name):
        if not condition:
            raise AssertionError(name)
        tests.append(name)

    def call(command, args=None, ok=True):
        nonlocal sequence
        sequence += 1
        proc.stdin.write(json.dumps(dict(id=sequence, command=command, args=args or {}), ensure_ascii=False) + "\n")
        proc.stdin.flush()
        reply = json.loads(replies.get(timeout=20))
        check(reply["id"] == sequence and reply["ok"] == ok, f"{sequence}_{command}_response")
        return reply.get("data") if ok else reply.get("error")

    try:
        library = call("library_get")
        check(library == {"lists": [], "presets": []}, "isolated_profile_starts_empty")
        library["presets"] = [dict(name="Keep preset", kind="Player", rules=[dict(key="ca", operation="set", value=100)])]
        library = call("library_put", dict(library=library, expected={"lists": [], "presets": []}))
        player = dict(kind="Player", id=101)
        staff = dict(kind="Staff", id=101)
        library = call("shortlist_update", dict(expected=library, change=dict(action="create", name=" 후보 ", members=[player, player])))
        check(library["lists"][0] == dict(name="후보", members=[player]), "trim_and_deduplicate")
        stale = copy.deepcopy(library)
        library = call("shortlist_update", dict(expected=library, change=dict(action="add", name="후보", members=[staff, player])))
        check(library["lists"][0]["members"] == [player, staff], "same_uid_distinct_kind")
        call("shortlist_update", dict(expected=stale, change=dict(action="delete", name="후보")), ok=False)
        check(call("library_get") == library, "stale_change_keeps_saved_data")
        call("library_put", dict(expected=stale, library=stale), ok=False)
        check(call("library_get") == library, "stale_whole_library_keeps_saved_data")
        call("shortlist_update", dict(expected=library, change=dict(action="create", name="후보", members=[])), ok=False)
        call("shortlist_update", dict(expected=library, change=dict(action="add", name="후보", members=[dict(kind="Player", id=0)])), ok=False)
        check(call("library_get") == library, "invalid_changes_are_atomic")
        library = call("shortlist_update", dict(expected=library, change=dict(action="rename", name="후보", new_name="영입 후보")))
        library = call("shortlist_update", dict(expected=library, change=dict(action="remove", name="영입 후보", members=[player])))
        check(library["lists"][0]["members"] == [staff], "remove_only_requested_identity")
        exported = profile / "export.json"
        call("library_export", dict(path=str(exported), name="영입 후보"))
        data = json.loads(exported.read_text(encoding="utf-8"))
        check(data == dict(lists=library["lists"], presets=[]), "selected_export_excludes_presets")
        incoming = call("library_import", dict(path=str(exported)))
        check(incoming == dict(library=data, saved=False), "import_validates_without_overwriting")
        check(call("library_get") == library, "import_preserves_existing_library")
        # FM file interchange remains offline, never mutates FM or silently imports.
        fm_staff = profile / "staff.fmf"
        response = call("fm_shortlist_export", dict(path=str(fm_staff), name="영입 후보", kind="Staff", expected=library))
        check(response["file_saved"] and not response["game_applied"], "file_write_is_not_game_apply")
        check(response["compatibility"] == "fm27_runtime_unverified", "runtime_limit_is_explicit")
        check(call("fm_shortlist_import", dict(path=str(fm_staff)))["library"] == data, "staff_fmf_import")
        before = fm_staff.read_bytes()
        call("fm_shortlist_export", dict(path=str(fm_staff), name="영입 후보", kind="Staff", expected=library), ok=False)
        check(fm_staff.read_bytes() == before, "existing_fmf_is_not_overwritten")
        stale_path = profile / "stale.fmf"
        call("fm_shortlist_export", dict(path=str(stale_path), name="영입 후보", kind="Staff", expected=stale), ok=False)
        check(not stale_path.exists(), "stale_export_writes_nothing")
        library = call("shortlist_update", dict(expected=library, change=dict(action="add", name="영입 후보", members=[player])))
        mixed_path = profile / "mixed.fmf"
        call("fm_shortlist_export", dict(path=str(mixed_path), name="영입 후보", kind="Player", expected=library), ok=False)
        check(not mixed_path.exists(), "mixed_export_writes_nothing")
        library = call("shortlist_update", dict(expected=library, change=dict(action="create", name="선수 🏆", members=[player, dict(kind="Player", id=2000123456)])))
        for ext in ("fmf", "slf"):
            path = profile / ("players." + ext)
            call("fm_shortlist_export", dict(path=str(path), name="선수 🏆", kind="Player", expected=library))
            incoming = call("fm_shortlist_import", dict(path=str(path)))
            check(incoming["library"]["lists"] == [library["lists"][1]] and not incoming["saved"], "player_" + ext + "_preview")
        # Independent native payload layout: 8-byte header, record byte, date,
        # UTF-8 byte count, count/UID32 sequence, end record. Not round-trip alone.
        name = "선수 🏆".encode("utf-8")
        expected_slf = b'\x03\x01fls.\x02\x00\x01' + struct.pack('<II', 0, len(name)) + name + struct.pack('<III', 2, 101, 2000123456) + b'\0'
        check((profile / "players.slf").read_bytes() == expected_slf, "native_record_byte_layout")
        old = profile / "old.slf"
        old_name = "옛 명단".encode("utf-16-le")
        old.write_bytes(b'\x03\x01fls.\x01\x00\x01' + struct.pack('<II', 0, len(old_name)//2) + old_name + b'\0\0' + struct.pack('<II', 1, 101) + b'\0')
        check(call("fm_shortlist_import", dict(path=str(old)))["library"]["lists"] == [dict(name="옛 명단", members=[player])], "independent_utf16_fixture")
        malformed = profile / "broken.fmf"
        malformed.write_bytes(before[:18])
        call("fm_shortlist_import", dict(path=str(malformed)), ok=False)
        check(call("library_get") == library, "fm_import_and_errors_preserve_library")
        library = call("shortlist_update", dict(expected=library, change=dict(action="delete", name="선수 🏆")))
        library = call("shortlist_update", dict(expected=library, change=dict(action="delete", name="영입 후보")))
        check(not library["lists"] and len(library["presets"]) == 1, "delete_preserves_presets")
        disk = json.loads((profile / "TouchlineEditor" / "scouting-library.json").read_text(encoding="utf-8"))
        check(disk == library, "disk_readback_matches_bridge")
        report.write_text(json.dumps(dict(passed=len(tests), failed=0, tests=tests, fm_accessed=False,
                                          user_profile_modified=False), ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"{len(tests)} checks passed; FM not accessed")
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()  # Only this test's child bridge, never the user's editor/FM.


if __name__ == "__main__":
    run(Path(sys.argv[1]), Path(sys.argv[2]))
