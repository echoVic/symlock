#!/usr/bin/env python3
"""Generate an asciinema v2 cast of the symlock demo with realistic pacing.

Runs the REAL symlock binary in a throwaway repo/worktrees and captures its
actual output, then lays it out as a typed terminal session. Output: cast.json
on stdout. Convert with:  svg-term --in cast.json --out demo.svg --window
"""
import json
import os
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.abspath(__file__))
BIN = os.path.join(ROOT, "..", "target", "release", "symlock")

# ANSI helpers
GREEN = "\033[32m"
CYAN = "\033[36m"
DIM = "\033[2m"
RST = "\033[0m"
PROMPT = f"{GREEN}${RST} "

events = []
t = 0.0


def emit(s: str, dt: float = 0.0):
    global t
    t += dt
    events.append([round(t, 3), "o", s])


def typed(cmd: str):
    """Emit a prompt then the command 'typed' char by char."""
    emit(PROMPT, 0.4)
    for ch in cmd:
        emit(ch, 0.035)
    emit("\r\n", 0.25)


def out(text: str, dt: float = 0.5):
    emit(text.replace("\n", "\r\n"), dt)


def sh(cmd, cwd, env):
    r = subprocess.run(cmd, cwd=cwd, env=env, shell=True,
                       capture_output=True, text=True)
    return (r.stdout + r.stderr), r.returncode


def main():
    if not os.path.exists(BIN):
        subprocess.run(["cargo", "build", "--release", "-q"], cwd=ROOT + "/..", check=True)

    work = tempfile.mkdtemp()
    repo = os.path.join(work, "repo")
    os.makedirs(repo)
    env = dict(os.environ, PATH=os.path.dirname(os.path.abspath(BIN)) + ":" + os.environ["PATH"])

    # Real setup
    sh("git init -q . && git config user.email d@d.co && git config user.name d", repo, env)
    with open(os.path.join(repo, "auth.ts"), "w") as f:
        f.write(
            "export function login(user: string, pass: string): boolean {\n"
            "  return check(user, pass);\n}\n\n"
            "export function logout(session: string): void {\n"
            "  destroy(session);\n}\n"
        )
    sh("git add -A && git commit -qm init", repo, env)
    sh(f"'{BIN}' init", repo, env)
    env["SYMLOCK_DIR"] = os.path.join(repo, ".symlock")
    sh("git worktree add -q ../agentA -b agentA && git worktree add -q ../agentB -b agentB", repo, env)
    wtA = os.path.join(work, "agentA")
    wtB = os.path.join(work, "agentB")

    # --- Script the narrative, capturing REAL output ---
    emit(f"{CYAN}# symlock: symbol-level conflict prevention for parallel AI agents{RST}\r\n", 0.2)
    emit(f"{DIM}# two agents, two git worktrees, one shared repo{RST}\r\n\r\n", 0.8)

    typed("symlock symbols auth.ts")
    o, _ = sh(f"'{BIN}' symbols auth.ts", repo, env)
    out(o, 0.4)
    out("\n")

    emit(f"{DIM}# agent A takes login, agent B takes logout — different functions{RST}\r\n", 0.6)
    typed("symlock claim auth.ts login       # agent A")
    o, _ = sh(f"SYMLOCK_AGENT=agentA '{BIN}' claim auth.ts login", wtA, env)
    out(o, 0.4)
    typed("symlock claim auth.ts logout      # agent B")
    o, _ = sh(f"SYMLOCK_AGENT=agentB '{BIN}' claim auth.ts logout", wtB, env)
    out(o, 0.4)
    out("\n")

    emit(f"{CYAN}# now agent B ALSO wants login — which A already holds{RST}\r\n", 0.6)
    typed("symlock claim auth.ts login       # agent B")
    o, rc = sh(f"SYMLOCK_AGENT=agentB '{BIN}' claim auth.ts login", wtB, env)
    out(o, 0.4)
    emit(f"{GREEN}exit {rc}{RST}  {DIM}← blocked before a single edit; no merge-time collision{RST}\r\n\r\n", 0.7)

    typed("symlock status")
    o, _ = sh(f"'{BIN}' status", repo, env)
    out(o, 0.4)
    emit("\r\n", 2.0)  # hold last frame

    header = {"version": 2, "width": 82, "height": 22,
              "theme": {"fg": "#c5c8c6", "bg": "#1d1f21"}}
    print(json.dumps(header))
    for e in events:
        print(json.dumps(e))
    subprocess.run(["rm", "-rf", work])


if __name__ == "__main__":
    main()
