#!/usr/bin/env python3
"""Tako Code: a separate, dependency-free PC runner. Python 3.11+, Git required."""
from __future__ import annotations
import argparse
import concurrent.futures
import contextlib
import getpass
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import shlex
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unicodedata
import urllib.error
import urllib.parse
import urllib.request

VERSION = "0.1.9"
TERMINAL = {"ready", "needs_attention", "failed", "cancelled", "applied"}
HOME = Path(os.environ.get("TAKO_CODE_HOME", Path.home() / ".config" / "tako-code"))
DENIED = {".git", ".env", ".env.local", ".env.production", ".ssh", ".aws", ".npmrc", ".pypirc"}
MAX_FILE = 100_000

class CodeError(Exception):
    pass

class Cancelled(CodeError):
    pass

def save(path: Path, value):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    tmp = path.with_suffix(".tmp")
    flags = os.O_CREAT | os.O_TRUNC | os.O_WRONLY
    fd = os.open(tmp, flags, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        json.dump(value, f, indent=2)
        f.write("\n")
    if os.name != "nt":
        tmp.chmod(0o600)
    os.replace(tmp, path)

def load(path: Path, default=None):
    if not path.exists():
        return {} if default is None else default
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        raise CodeError(f"Cannot read {path}: {e}") from None

def config():
    c = load(HOME / "config.json")
    if not c.get("url") or not c.get("key"):
        raise CodeError("Run tako-code setup first")
    return c

def validate_url(url: str):
    url = url.strip().rstrip("/")
    if url.endswith("/v1"):
        url = url[:-3]
    u = urllib.parse.urlsplit(url)
    if u.username or u.password or u.query or u.fragment or not u.hostname:
        raise CodeError("Use a VPS URL without credentials, query or fragment")
    local = u.hostname in {"localhost", "127.0.0.1", "::1"}
    if u.scheme != "https" and not (u.scheme == "http" and local):
        raise CodeError("Use HTTPS for your VPS (HTTP is allowed only on loopback)")
    return url

class SameOriginRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        old, new = urllib.parse.urlsplit(req.full_url), urllib.parse.urlsplit(newurl)
        if (old.scheme, old.netloc) != (new.scheme, new.netloc):
            raise CodeError("VPS redirected to a different origin; use its canonical HTTPS URL")
        return super().redirect_request(req, fp, code, msg, headers, newurl)

class Api:
    def __init__(self, c):
        self.c = c
        self.url = validate_url(c["url"])
        raw = c["key"]
        self.key = os.environ.get(raw[4:], "") if raw.startswith("env:") else raw
        if not self.key:
            raise CodeError("The configured Tako client key is missing")

    def redact(self, text):
        for value in [self.key, self.c.get("runner_token", "")]:
            if value:
                text = text.replace(value, "[redacted]")
        return text

    def request(self, path, value=None, timeout=90):
        body = json.dumps(value).encode() if value is not None else None
        req = urllib.request.Request(self.url + path, data=body, headers={
            "Authorization": "Bearer " + self.key, "Content-Type": "application/json",
            "User-Agent": "tako-code/" + VERSION,
        })
        try:
            with urllib.request.build_opener(SameOriginRedirect()).open(req, timeout=timeout) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            try:
                message = json.load(e).get("error", {})
                message = message.get("message", str(message)) if isinstance(message, dict) else str(message)
            except (ValueError, OSError):
                message = "Request rejected"
            raise CodeError(self.redact(f"VPS HTTP {e.code}: {message}")) from None
        except (urllib.error.URLError, TimeoutError, ValueError) as e:
            raise CodeError(self.redact(f"Cannot reach Tako VPS: {e}")) from None

    def complete(self, messages, tools=None, reasoning=False):
        body = {"model": "free:reasoning" if reasoning else "free:coding", "messages": messages,
                "max_tokens": 4096, "stream": False}
        if tools:
            body["tools"] = tools
        v = self.request("/code/v1/chat/completions", body, timeout=180)
        try:
            return v["choices"][0]["message"]
        except (KeyError, IndexError, TypeError):
            raise CodeError("Free-model gateway returned no assistant message") from None

    def runner(self, action, **values):
        return self.request("/v1/code/" + action, {
            "runner": self.c["runner"], "runner_token": self.c.get("runner_token", ""), **values,
        }, timeout=30)

def git(repo, *args, check=True):
    # Hooks must not run as a side effect of machine-generated commits or merges.
    cmd = ["git", "-c", "core.hooksPath=" + str(HOME / "empty-hooks"), "-c", "core.fsmonitor=false", "-c", "commit.gpgSign=false",
           "-c", "user.name=Tako Code", "-c", "user.email=tako-code@localhost", "-C", str(repo), *args]
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
    if check and p.returncode:
        raise CodeError("git " + args[0] + ": " + p.stderr.strip()[:2000])
    return p

def repo_root(path):
    if not shutil.which("git"):
        raise CodeError("Git is required on the PC")
    root = Path(git(Path(path).resolve(), "rev-parse", "--show-toplevel").stdout.strip()).resolve()
    git(root, "rev-parse", "HEAD")
    return root

def safe_path(root: Path, relative: str, writing=False):
    if not isinstance(relative, str) or not relative or Path(relative).is_absolute():
        raise CodeError("Use a relative path inside the worker worktree")
    parts = Path(relative).parts
    # macOS and Windows file systems ignore case (and Windows trailing dots/spaces), so
    # ".GIT" or ".env. " must be treated like ".git" and ".env"; 8.3 names hide them too.
    # HFS+/APFS also ignore invisible format characters (".g‌it"), as in CVE-2014-9390.
    names = ["".join(ch for ch in unicodedata.normalize("NFKC", p) if unicodedata.category(ch) != "Cf")
             .casefold().rstrip(". ") for p in parts]
    if ".." in parts or any(n in DENIED or n.startswith(".env.") or n.endswith((".pem", ".key"))
                            or re.search(r"~\d", n) or ":" in n for n in names):
        raise CodeError("This path is outside the permitted coding files")
    path = root.joinpath(relative)
    # Do not follow a symlink at any level, including one pointing into the repo.
    cursor = root
    for part in parts:
        cursor = cursor / part
        if cursor.is_symlink():
            raise CodeError("Symlink access is disabled for coding tools")
    if not path.resolve().is_relative_to(root.resolve()):
        raise CodeError("Path escapes the worker worktree")
    # Identity check as well as names: whatever spelling reaches .git on disk is refused.
    cursor = root
    for part in parts:
        cursor = cursor / part
        if cursor.exists() and (root / ".git").exists() and os.path.samefile(cursor, root / ".git"):
            raise CodeError("This path is outside the permitted coding files")
    if writing and path.exists() and not path.is_file():
        raise CodeError("Only regular files can be edited")
    return path

def tracked_files(root):
    result = git(root, "ls-files", "--cached", "--others", "--exclude-standard", "-z").stdout
    files = []
    for name in result.split("\0"):
        if name:
            try:
                safe_path(root, name)
                files.append(name)
            except CodeError:
                pass
    return files[:3000]

def run_tests(root, commands, cancelled, log):
    results = []
    for command in commands:
        if cancelled.is_set():
            raise Cancelled("Cancelled")
        log("Running configured test: " + " ".join(command))
        env = child_env()
        env["PYTHONDONTWRITEBYTECODE"] = "1"
        with tempfile.TemporaryFile() as output:
            p = subprocess.Popen(command, cwd=root, env=env, stdout=output, stderr=subprocess.STDOUT,
                                 start_new_session=os.name != "nt")
            deadline = time.monotonic() + 300
            while p.poll() is None:
                if cancelled.wait(0.25) or time.monotonic() > deadline:
                    terminate(p)
                    raise CodeError("Test cancelled or exceeded five minutes")
            output.seek(0)
            content = output.read(8000).decode("utf-8", "replace")
        results.append({"command": command, "exit_code": p.returncode, "output": content})
        if p.returncode:
            break
    return results

def terminate(p):
    if p.poll() is not None:
        return
    try:
        if os.name == "nt":
            subprocess.run(["taskkill", "/PID", str(p.pid), "/T", "/F"], capture_output=True, timeout=10)
        else:
            os.killpg(p.pid, signal.SIGTERM)
        p.wait(timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        p.kill()


def child_env():
    # Do not forward unrelated provider tokens, cloud keys, or shell startup options.
    allowed = {"PATH", "SystemRoot", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT", "TEMP", "TMP", "TMPDIR",
               "LANG", "LC_ALL", "TERM", "HOME", "RUSTUP_HOME", "CARGO_HOME", "VIRTUAL_ENV", "USERPROFILE", "LOCALAPPDATA", "APPDATA"}
    return {k: v for k, v in os.environ.items() if k in allowed}


def tool(name, description, properties, required=()):
    return {"type": "function", "function": {"name": name, "description": description,
        "parameters": {"type": "object", "properties": properties, "required": list(required), "additionalProperties": False}}}

TOOLS = [
    tool("list_files", "List coding files in this isolated worktree", {}),
    tool("read_file", "Read a relative UTF-8 source file", {"path": {"type": "string"}}, ["path"]),
    tool("write_file", "Create or replace a UTF-8 source file; use read_file first for existing files",
         {"path": {"type": "string"}, "content": {"type": "string"}}, ["path", "content"]),
    tool("search", "Search a literal string in coding files", {"text": {"type": "string"}}, ["text"]),
    tool("diff", "Show your changes in this worktree", {}),
]

class Builtin:
    def __init__(self, api, root, cancel, log, turns=30):
        self.api, self.root, self.cancel, self.log, self.turns = api, root, cancel, log, turns
        self.read = set()

    def execute(self, name, args):
        if name == "list_files":
            return "\n".join(tracked_files(self.root))[:40000]
        if name == "read_file":
            path = safe_path(self.root, args["path"])
            if path.stat().st_size > MAX_FILE:
                raise CodeError("File is too large; select a smaller source file")
            self.read.add(args["path"])
            return path.read_text(encoding="utf-8")
        if name == "write_file":
            path = safe_path(self.root, args["path"], writing=True)
            if path.exists() and args["path"] not in self.read:
                raise CodeError("Read this file before replacing it")
            content = args["content"]
            if not isinstance(content, str) or len(content.encode()) > MAX_FILE:
                raise CodeError("File exceeds the coding tool size limit")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
            return "Saved " + args["path"]
        if name == "search":
            needle = args["text"]
            if not isinstance(needle, str) or not needle or len(needle) > 300:
                raise CodeError("Search needs a short literal string")
            hits = []
            for name in tracked_files(self.root)[:1000]:
                try:
                    path = safe_path(self.root, name)
                    if path.stat().st_size > MAX_FILE:
                        continue
                    for i, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                        if needle in line:
                            hits.append(f"{name}:{i}: {line[:300]}")
                        if len(hits) >= 60:
                            return "\n".join(hits)
                except (OSError, UnicodeError, CodeError):
                    continue
            return "\n".join(hits) or "No matches"
        if name == "diff":
            return git(self.root, "diff", "HEAD", "--").stdout[:50000]
        raise CodeError("Unknown tool")

    def run(self, prompt):
        context = "\n".join(tracked_files(self.root)[:400])
        instructions = ""
        for filename in ["AGENTS.md", "CLAUDE.md"]:
            try:
                instructions += safe_path(self.root, filename).read_text(encoding="utf-8")[:12000] + "\n"
            except (OSError, UnicodeError, CodeError):
                pass
        messages = [{"role": "system", "content": "You are a Tako coding worker in an isolated Git worktree. "
            "Implement only your assigned task. Read existing files before replacing them. Respect repository instructions. "
            "Do not access secrets, .git metadata, or external systems. Do not claim tests ran; the coordinator runs configured tests. "
            "Use tools to actually edit code. End with a concise summary of edits and limitations.\nRepository instructions:\n" + instructions},
            {"role": "user", "content": prompt + "\nAvailable files:\n" + context}]
        deadline = time.monotonic() + 1200
        for _ in range(self.turns):
            if time.monotonic() > deadline:
                raise CodeError("Worker exceeded its 20-minute limit")
            if self.cancel.is_set():
                raise Cancelled("Cancelled")
            message = self.api.complete(messages, TOOLS)
            message["role"] = "assistant"
            messages.append(message)
            if not message.get("tool_calls"):
                answer = message.get("content") or "Worker finished without a summary"
                self.log(str(answer)[:1800])
                return str(answer)
            for call in message["tool_calls"]:
                if self.cancel.is_set():
                    raise Cancelled("Cancelled")
                name = call.get("function", {}).get("name", "")
                try:
                    args = json.loads(call["function"]["arguments"])
                    self.log(name + (" " + str(args.get("path", "")) if args.get("path") else ""))
                    result = self.execute(name, args)
                except (CodeError, OSError, UnicodeError, ValueError, KeyError, TypeError) as e:
                    result = "Tool error: " + str(e)
                messages.append({"role": "tool", "tool_call_id": call["id"], "content": result[:50000]})
        raise CodeError("Worker reached its 30-turn limit; inspect its preserved worktree")


def external(engine, api, root, prompt, cancel, log):
    executable = shutil.which(engine)
    if not executable or (os.name == "nt" and executable.lower().endswith((".cmd", ".bat"))):
        raise CodeError(f"Install {engine} or select the built-in engine")
    with tempfile.TemporaryDirectory(prefix="tako-code-engine-") as private:
        env = child_env()
        env.update({"HOME": private, "USERPROFILE": private, "XDG_CONFIG_HOME": private,
                    "XDG_DATA_HOME": private, "XDG_CACHE_HOME": private, "CI": "1"})
        # All model traffic goes to the server-enforced free gateway, regardless of model names.
        if engine == "opencode":
            spec = {"$schema": "https://opencode.ai/config.json", "enabled_providers": ["tako"],
                "model": "tako/free:coding", "small_model": "tako/free:coding",
                "provider": {"tako": {"npm": "@ai-sdk/openai-compatible", "name": "Tako free gateway",
                    "options": {"baseURL": api.url + "/code/v1", "apiKey": api.key},
                    "models": {"free:coding": {"name": "Tako free coding", "limit": {"context": 64000, "output": 4096}}}}},
                "permission": {"*": "deny", "read": "allow", "edit": "allow", "glob": "allow", "grep": "allow", "list": "allow",
                    "external_directory": "deny", "bash": "deny", "webfetch": "deny", "websearch": "deny"}}
            env.update({"OPENCODE_CONFIG_CONTENT": json.dumps(spec), "OPENCODE_DISABLE_AUTOUPDATE": "1", "OPENCODE_DISABLE_SHARE": "1"})
            command = [executable, "run", "--pure", "--format", "json", "--model", "tako/free:coding", prompt]
        elif engine == "claude":
            env.update({"ANTHROPIC_BASE_URL": api.url + "/code", "ANTHROPIC_API_KEY": api.key,
                        "ANTHROPIC_AUTH_TOKEN": api.key, "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
                        "ANTHROPIC_CUSTOM_MODEL_OPTION": "free:coding", "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME": "Tako free coding"})
            for key in ["ANTHROPIC_MODEL", "ANTHROPIC_DEFAULT_OPUS_MODEL", "ANTHROPIC_DEFAULT_SONNET_MODEL", "ANTHROPIC_DEFAULT_HAIKU_MODEL", "ANTHROPIC_SMALL_FAST_MODEL"]:
                env[key] = "free:coding"
            deny = [f"{tool}({pattern})" for tool in ["Read", "Edit", "Write", "Glob", "Grep"]
                    for pattern in ["./.env*", "./**/.env*", "./.git/**", "./**/.git/**", "./**/*.pem", "./**/*.key"]]
            settings = json.dumps({"permissions": {"deny": deny}})
            command = [executable, "--bare", "--restricted", "--settings", settings, "--max-turns", "30", "-p", "--output-format", "stream-json", "--verbose", "--model", "free:coding",
                       "--permission-mode", "acceptEdits", "--tools", "Read,Write,Edit,Glob,Grep", "--allowedTools", "Read,Write,Edit,Glob,Grep", "--", prompt]
        elif engine == "codex":
            # A private CODEX_HOME keeps the user's ChatGPT login and config out of the worker;
            # the custom provider sends every request to the free-only Responses gateway.
            env.update({"CODEX_HOME": private, "TAKO_CODE_GATEWAY_KEY": api.key})
            provider = {"name": "Tako free gateway", "base_url": api.url + "/code/v1", "env_key": "TAKO_CODE_GATEWAY_KEY",
                        "wire_api": "responses", "request_max_retries": 4, "stream_max_retries": 4, "stream_idle_timeout_ms": 600000}
            settings = ['model_provider="tako"', 'approval_policy="never"', 'web_search="disabled"',
                        "sandbox_workspace_write.network_access=false", 'shell_environment_policy.inherit="core"']
            settings += [f"model_providers.tako.{k}={json.dumps(v)}" for k, v in provider.items()]
            command = [executable, "exec", "--json", "--ephemeral", "--skip-git-repo-check", "--ignore-rules", "--color", "never",
                       "--sandbox", "workspace-write", "--model", "free:coding", "-C", str(root)]
            for setting in settings:
                command += ["-c", setting]
            command += ["--", prompt]
        else:
            raise CodeError("Unknown engine; use builtin, opencode, claude or codex.")
        output = Path(private) / "events.log"
        with output.open("w", encoding="utf-8") as f:
            p = subprocess.Popen(command, cwd=root, env=env, stdin=subprocess.DEVNULL, stdout=f, stderr=subprocess.STDOUT,
                                 start_new_session=os.name != "nt")
        offset, pending = 0, ""
        deadline = time.monotonic() + 1200
        while True:
            with output.open(encoding="utf-8", errors="replace") as f:
                f.seek(offset)
                chunk = f.read(64000)
                offset = f.tell()
            pending += chunk
            lines = pending.split("\n")
            pending = lines.pop()
            for line in lines[-50:]:
                if line.strip():
                    # Only emit bounded, redacted output. Do not send engine environment/configs.
                    log(api.redact(line[:1800]))
            if p.poll() is not None:
                if p.returncode:
                    raise CodeError(f"{engine} exited with {p.returncode}; inspect the worker events")
                return f"{engine} finished"
            if cancel.wait(0.5) or time.monotonic() > deadline:
                terminate(p)
                raise Cancelled("Worker cancelled or reached its 20-minute limit")

class Swarm:
    def __init__(self, api, job, project, cancel):
        self.api, self.job, self.project, self.cancel = api, job, project, cancel
        if not re.fullmatch(r"[a-f0-9]{16}", str(job.get("id", ""))):
            raise CodeError("Invalid job id")
        self.repo = repo_root(project["path"])
        if HOME.resolve().is_relative_to(self.repo):
            raise CodeError("Keep TAKO_CODE_HOME outside the registered repository")
        self.folder = HOME / "jobs" / job["id"]
        self.folder.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.state = {"id": job["id"], "runner": api.c["runner"], "repo": str(self.repo), "phase": "planning", "workers": [],
                      "task": job.get("task", ""), "engine": job.get("engine", "")}
        self.lock = threading.RLock()
        self.pending = []
        self.listener = None  # the terminal UI watches events here
        self.thread = None

    def event(self, message="", **values):
        with self.lock:
            message = self.api.redact(str(message))[:2000]
            for key, value in list(values.items()):
                values[key] = json.loads(self.api.redact(json.dumps(value)))
            self.state.update({k: v for k, v in values.items() if k != "worker"})
            save(self.folder / "state.json", self.state)
            event = {"message": message, **values}
            self.pending.append(event)
            if self.listener:
                try:
                    self.listener(message, values)
                except Exception:
                    pass  # a display problem must never stop the swarm
            self.flush()

    def flush(self):
        # Keep bounded local events if the VPS connection drops; retry on heartbeat.
        with self.lock:
            self.pending = self.pending[-100:]
            while self.pending:
                try:
                    result = self.api.runner("event", job=self.job["id"], event=self.pending[0])
                except CodeError:
                    break
                self.pending.pop(0)
                if result.get("cancel"):
                    self.cancel.set()

    def plan(self):
        self.event("Planning independent tasks", phase="planning")
        instructions = ("Split this coding request into independent, non-overlapping implementation tasks. "
            "Return ONLY a JSON array with 1 to " + str(self.job["workers_requested"]) + " objects. "
            'Each object has title and instructions strings. Use fewer tasks if work cannot be split. '
            "Do not assign two workers to the same files. No deployment, credentials or paid APIs.\nRequest:\n" + self.job["task"] +
            "\nRepository files:\n" + "\n".join(tracked_files(self.repo)[:500]))
        response = self.api.complete([{"role": "user", "content": instructions}], reasoning=True).get("content", "") or ""
        response = re.sub(r"^```(?:json)?\s*|\s*```$", "", response.strip())
        try:
            tasks = json.loads(response)
            if not isinstance(tasks, list) or not 1 <= len(tasks) <= self.job["workers_requested"]:
                raise ValueError()
            if not all(isinstance(t, dict) and isinstance(t.get("title"), str) and isinstance(t.get("instructions"), str) and t["instructions"].strip() for t in tasks):
                raise ValueError()
        except (ValueError, TypeError):
            raise CodeError("Planner did not return valid independent tasks; no workers were launched") from None
        return tasks

    def worker(self, index, task, base):
        branch = f"tako-code/{self.job['id']}/worker-{index}"
        root = self.folder / f"worker-{index}"
        # Git's worktree metadata is not safe for concurrent `worktree add` on one repo.
        with self.lock:
            git(self.repo, "worktree", "add", "-b", branch, str(root), base)
        worker = {"id": index, "title": task["title"][:100], "phase": "working", "branch": branch}
        with self.lock:
            self.state["workers"].append(worker)
            self.event("Worker started: " + worker["title"], workers=self.state["workers"], worker=index)
        log = lambda message: self.event(message, worker=index)
        try:
            prompt = ("Overall request: " + self.job["task"] + "\nYour assigned task: " + task["instructions"] +
                      "\nOther workers have different assignments. Do not commit, merge, deploy or run network commands. "
                      "Edit files only in this worktree; the coordinator will commit and test.")
            if self.job["engine"] == "builtin":
                Builtin(self.api, root, self.cancel, log).run(prompt)
            else:
                external(self.job["engine"], self.api, root, prompt, self.cancel, log)
            if self.cancel.is_set():
                raise Cancelled("Cancelled")
            if not git(root, "status", "--porcelain").stdout.strip():
                worker["phase"] = "no_changes"
                worker["commit"] = None
            else:
                git(root, "add", "-A")
                git(root, "commit", "-m", "Tako Code: " + task["title"][:100])
                worker["commit"] = git(root, "rev-parse", "HEAD").stdout.strip()
                worker["phase"] = "done"
            return worker
        except Exception as e:
            worker["phase"] = "cancelled" if isinstance(e, Cancelled) else "failed"
            worker["error"] = self.api.redact(str(e))[:2000]
            raise
        finally:
            with self.lock:
                self.state["workers"] = [worker if w["id"] == index else w for w in self.state["workers"]]
                self.event("Worker " + worker["phase"], workers=self.state["workers"], worker=index)

    def run(self):
        try:
            if git(self.repo, "status", "--porcelain").stdout.strip():
                raise CodeError("Commit or stash local changes before starting a swarm; nothing was modified")
            base = git(self.repo, "rev-parse", "HEAD").stdout.strip()
            self.state["base"] = base
            tasks = self.plan()
            self.event(f"Launching {len(tasks)} isolated workers", phase="working", base=base)
            with concurrent.futures.ThreadPoolExecutor(max_workers=len(tasks)) as pool:
                futures = [pool.submit(self.worker, i + 1, task, base) for i, task in enumerate(tasks)]
                results, failures = [], []
                for future in concurrent.futures.as_completed(futures):
                    try:
                        results.append(future.result())
                    except Exception as e:
                        failures.append(str(e))
                if self.cancel.is_set():
                    raise Cancelled("Cancelled; worktrees preserved")
                if failures:
                    raise CodeError("Worker failed; all worktrees preserved: " + failures[0])
            commits = [w["commit"] for w in sorted(results, key=lambda w: w["id"]) if w.get("commit")]
            if not commits:
                raise CodeError("Workers produced no code changes")
            branch = f"tako-code/{self.job['id']}/review"
            merged = self.folder / "review"
            git(self.repo, "worktree", "add", "-b", branch, str(merged), base)
            self.event("Combining worker commits on a review branch", phase="integrating", branch=branch)
            for commit in commits:
                result = git(merged, "cherry-pick", commit, check=False)
                if result.returncode:
                    self.event("Merge conflict; review worktree preserved for manual resolution", phase="needs_attention", branch=branch,
                               error="Worker changes conflict; no changes were applied to your checkout.")
                    return
            diff = git(merged, "diff", "--no-ext-diff", base, "HEAD", "--").stdout
            (self.folder / "changes.patch").write_text(diff, encoding="utf-8")
            self.event("Reviewing combined changes", phase="reviewing", diff=diff[:50000], diff_truncated=len(diff)>50000, branch=branch)
            tests = run_tests(merged, self.project.get("test_commands", []), self.cancel, lambda s: self.event(s))
            tests = json.loads(self.api.redact(json.dumps(tests)))
            review = self.api.complete([{"role": "system", "content": "Review this patch for correctness, missed requirements and security bugs. Be concise. You cannot edit files or claim tests passed."},
                {"role": "user", "content": "Task:\n" + self.job["task"] + "\nPatch:\n" + diff[:60000] + "\nActual test results:\n" + json.dumps(tests)[:10000]}], reasoning=True).get("content") or "No review text returned"
            dirty = bool(git(merged, "status", "--porcelain", "--untracked-files=no").stdout.strip())
            failed = dirty or any(t["exit_code"] for t in tests)
            if dirty:
                review = "Configured tests modified the review worktree; inspect those changes before applying.\n" + str(review)
            self.event("Tests failed; inspect review branch" if failed else "Changes ready for your review",
                phase="needs_attention" if failed else "ready", review=str(review)[:40000], tests=tests,
                apply_command=(subprocess.list2cmdline if os.name == "nt" else shlex.join)([*self_command(), "apply", self.job["id"]]))
        except Cancelled as e:
            self.event(str(e), phase="cancelled")
        except Exception as e:
            self.event(str(e), phase="failed", error=self.api.redact(str(e))[:4000])
        finally:
            self.flush()


def self_command():
    # A standalone executable is its own interpreter; a script needs Python in front.
    if getattr(sys, "frozen", False):
        return [sys.executable]
    return [sys.executable, str(Path(sys.argv[0]).resolve())]

def available_engines():
    return ["builtin"] + [e for e in ["opencode", "claude", "codex"] if shutil.which(e) and not (os.name == "nt" and shutil.which(e).lower().endswith((".cmd", ".bat")))]

def projects(c):
    return [{"id": k, "name": p["name"]} for k, p in c.get("projects", {}).items()]

def register(api, c):
    result = api.runner("register", name=c["name"], projects=projects(c), engines=available_engines())
    if result.get("runner_token"):
        c["runner_token"] = result["runner_token"]
        save(HOME / "config.json", c)
        api.c = c
    # Registration intentionally takes two requests so a new secret is persisted first.
    api.runner("register", name=c["name"], projects=projects(c), engines=available_engines())

@contextlib.contextmanager
def runner_lock():
    HOME.mkdir(parents=True, exist_ok=True, mode=0o700)
    f = (HOME / "runner.lock").open("a+b")
    try:
        if os.name == "nt":
            import msvcrt
            f.write(b"0"); f.flush(); f.seek(0)
            msvcrt.locking(f.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(f.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        f.close()
        raise CodeError("A runner is already running for this configuration") from None
    try:
        yield
    finally:
        f.close()

class Runner:
    """The PC's connection to the VPS: polls for web jobs and runs one swarm at a time."""

    def __init__(self, c):
        self.c, self.api = c, Api(c)
        self.active = None
        self.lock = threading.RLock()
        self.stop = threading.Event()
        self.on_job = None  # called with a swarm started from the web panel
        self.error = None

    def connect(self):
        cap = self.api.request("/v1/code/capabilities")
        if cap.get("protocol") != 1 or not cap.get("free_gateway"):
            raise CodeError("Update your VPS Tako server; its free-only Code gateway is missing")
        register(self.api, self.c)
        self.api.runner("recover")
        for local in (HOME / "jobs").glob("*/state.json"):
            state = load(local)
            if state.get("runner") != self.c["runner"]:
                continue
            if state.get("phase") in TERMINAL:
                try:
                    self.api.runner("event", job=state["id"], event={k: v for k, v in state.items() if k not in {"id", "repo", "runner", "task", "engine"}})
                except CodeError:
                    pass
            else:
                state["phase"] = "failed"
                state["error"] = "Runner was interrupted; worktrees are preserved"
                save(local, state)

    def busy(self):
        a = self.active
        return a is not None and (a.thread.is_alive() or bool(a.pending))

    def _start(self, job, project):
        swarm = Swarm(self.api, job, project, threading.Event())
        swarm.thread = threading.Thread(target=swarm.run, daemon=False)
        self.active = swarm
        swarm.thread.start()
        return swarm

    def tick(self):
        """One poll of the VPS. Returns a swarm if a web job was started."""
        with self.lock:
            response = self.api.runner("poll", name=self.c["name"], projects=projects(self.c),
                                       engines=available_engines(), busy=self.busy())
            if self.active:
                if self.active.job["id"] in response.get("cancel", []):
                    self.active.cancel.set()
                self.active.flush()
                if not self.busy():
                    self.active = None
            job = response.get("job")
            if not job:
                return None
            # Job ids become local folder and branch names; never trust the VPS with paths.
            if not re.fullmatch(r"[a-f0-9]{16}", str(job.get("id", ""))):
                raise CodeError("VPS sent an invalid job id; ignoring it")
            project = self.c.get("projects", {}).get(job.get("project"))
            if not project:
                self.api.runner("event", job=job["id"], event={"phase": "failed", "message": "Project is not registered on this PC"})
            elif self.active:
                self.api.runner("event", job=job["id"], event={"phase": "failed", "message": "Runner is already working on another job"})
            else:
                try:
                    return self._start(job, project)
                except (CodeError, OSError) as e:
                    self.api.runner("event", job=job["id"], event={"phase": "failed", "message": self.api.redact(str(e))[:2000]})
            return None

    def submit(self, project_id, engine, workers, task):
        """Start a swarm typed on this PC; the VPS records it so the web UI shows it too."""
        with self.lock:
            if self.busy():
                raise CodeError("A swarm is already running; wait for it or cancel it first")
            # Re-announce projects and engines so a just-added repository is accepted.
            self.api.runner("register", name=self.c["name"], projects=projects(self.c), engines=available_engines())
            job = self.api.runner("submit", project=project_id, engine=engine, workers=workers, task=task)["job"]
            if not re.fullmatch(r"[a-f0-9]{16}", str(job.get("id", ""))):
                raise CodeError("VPS sent an invalid job id")
            return self._start(job, self.c["projects"][project_id])

    def background(self):
        while not self.stop.wait(2):
            try:
                swarm = self.tick()
                self.error = None
                if swarm and self.on_job:
                    self.on_job(swarm)
            except CodeError as e:
                self.error = str(e)

    def shutdown(self, wait=190):
        self.stop.set()
        a = self.active
        if a and a.thread.is_alive():
            a.cancel.set()
            a.thread.join(timeout=wait)
        if a:
            a.flush()

def run(c, once=False):
    runner = Runner(c)
    runner.connect()
    print("Tako Code connected to " + runner.api.url + ". Keep this terminal running; start swarms from the web panel.", flush=True)
    try:
        while True:
            try:
                runner.tick()
            except CodeError as e:
                print(clean(e), file=sys.stderr, flush=True)
                if once:
                    raise
            if once and not runner.active:
                return
            runner.stop.wait(2)
    except KeyboardInterrupt:
        if runner.active and runner.active.thread.is_alive():
            print("Stopping workers; waiting for any current model request to finish…", flush=True)
        runner.shutdown()
        print("Runner stopped; worktrees and local job state are preserved.")

def apply(job_id):
    if not re.fullmatch(r"[a-f0-9]{16}", job_id):
        raise CodeError("Invalid job ID")
    state = load(HOME / "jobs" / job_id / "state.json")
    if state.get("phase") != "ready":
        raise CodeError("Only a reviewed, ready job can be applied")
    root = repo_root(state["repo"])
    if git(root, "status", "--porcelain").stdout.strip():
        raise CodeError("Your checkout has local changes; commit or stash them first")
    if git(root, "rev-parse", "HEAD").stdout.strip() != state["base"]:
        raise CodeError("Your branch moved since the swarm started; merge the review branch manually")
    git(root, "merge", "--ff-only", state["branch"])
    state["phase"] = "applied"
    save(HOME / "jobs" / job_id / "state.json", state)
    api = Api(config())
    try:
        api.runner("event", job=job_id, event={"phase": "applied", "message": "Applied on local PC with a fast-forward merge"})
    except CodeError:
        print("Applied locally; VPS status will need a refresh when connected")
    print("Applied " + state["branch"])


# ---- terminal UI: `tako-code` with no arguments ---------------------------------

ENGINE_NAMES = {"claude": "Claude Code", "codex": "Codex", "opencode": "OpenCode", "builtin": "Built-in agent"}
ENGINE_ORDER = ["claude", "codex", "opencode", "builtin"]
SPINNER = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"

def paint(code, text):
    return f"\x1b[{code}m{text}\x1b[0m" if USE_COLOR else text

USE_COLOR = sys.stdout.isatty() and "NO_COLOR" not in os.environ

def enable_terminal():
    if os.name == "nt":
        os.system("")  # turns on ANSI escape handling in the Windows console
    with contextlib.suppress(AttributeError, ValueError):
        sys.stdout.reconfigure(errors="replace")

CONTROL = re.compile(r"[\x00-\x08\x0b-\x1f\x7f-\x9f]")
CONTROL_LINE = re.compile(r"[\x00-\x1f\x7f-\x9f]")

def clean(text, multiline=False):
    """Strip terminal control characters (ESC, CSI, BEL…) from model, agent, test or server text.

    Without this, a model could print escape sequences that rewrite the screen or set the clipboard.
    """
    text = str(text).replace("\t", "    ")
    return CONTROL.sub("", text) if multiline else CONTROL_LINE.sub(" ", text)

def short_path(value):
    parts = Path(str(value)).parts
    return str(Path(*parts[-2:])) if len(parts) > 2 else str(value)

def first_line(text, limit=120):
    for line in str(text).splitlines():
        if line.strip():
            return clean(line.strip())[:limit]
    return ""

def activity(line):
    """Turn a worker log line (plain text or an agent CLI's JSON event) into a short status."""
    line = str(line).strip()
    if not line:
        return None
    if not line.startswith("{"):
        return first_line(line)
    try:
        v = json.loads(line)
    except ValueError:
        return first_line(line)
    if not isinstance(v, dict):
        return None
    kind = v.get("type")
    if kind == "assistant":  # Claude Code stream-json
        for part in (v.get("message") or {}).get("content") or []:
            if part.get("type") == "tool_use":
                args = part.get("input") or {}
                target = args.get("file_path") or args.get("path") or args.get("pattern") or args.get("command") or ""
                return f"{part.get('name') or 'tool'} {short_path(target)}".strip()
            if part.get("type") == "text" and str(part.get("text", "")).strip():
                return first_line(part["text"])
        return None
    if kind == "result":
        return "stopped with an error" if v.get("is_error") else "finished"
    item = v.get("item")
    if isinstance(item, dict):  # Codex exec --json
        it = item.get("type")
        if it == "command_execution":
            return "$ " + first_line(item.get("command", ""), 100)
        if it == "file_change":
            return "edited " + ", ".join(short_path(c.get("path", "")) for c in item.get("changes", []) if isinstance(c, dict))
        if it in ("agent_message", "reasoning") and item.get("text"):
            return first_line(item["text"])
        return None
    part = v.get("part") if isinstance(v.get("part"), dict) else {}
    if kind == "tool_use" or part.get("type") == "tool":  # OpenCode --format json
        args = (part.get("state") or {}).get("input") or {}
        return f"{part.get('tool', 'tool')} {short_path(args.get('filePath') or args.get('path') or '')}".strip()
    if kind == "text" and part.get("text"):
        return first_line(part["text"])
    return None

def clip(text, width):
    text = clean(text)
    return text if len(text) <= width else text[:max(0, width - 1)] + "…"

def elapsed(seconds):
    seconds = int(seconds)
    return f"{seconds // 60}m {seconds % 60:02d}s" if seconds >= 60 else f"{seconds}s"

def board(state, acts, notes, since, frame=0, width=80, running=True):
    """Lines for the live swarm view. Pure, so it can be tested without a terminal."""
    width = max(40, width) - 1
    phase = state.get("phase", "planning")
    engine = ENGINE_NAMES.get(state.get("engine"), state.get("engine") or "agents")
    workers = state.get("workers") or []
    head = f"{engine} × {len(workers)}" if workers else engine
    lines = [paint("1", clip("▸ " + state.get("task", ""), width)),
             paint("2", clip(f"  {head} · {phase.replace('_', ' ')} · {elapsed(time.monotonic() - since)}", width)), ""]
    if not workers:
        lines.append(f"  {paint('36', SPINNER[frame % len(SPINNER)])} Planning: splitting the task between agents…")
    for w in workers:
        p = w.get("phase")
        mark, tint = {"done": ("✓", "32"), "no_changes": ("–", "2"), "failed": ("✗", "31"),
                      "cancelled": ("⊘", "33")}.get(p, (SPINNER[(frame + w.get("id", 0)) % len(SPINNER)], "36"))
        detail = w.get("error") or {"done": "done", "no_changes": "no changes"}.get(p) or acts.get(w.get("id")) or "starting…"
        # Measure plain text first so colors never push a line past the terminal width.
        rest = clip(f" {w.get('id')}  {clip(w.get('title', ''), 26).ljust(26)}  ", max(0, width - 3))
        lines.append("  " + paint(tint, mark) + rest + paint("2", clip(detail, max(0, width - 3 - len(rest)))))
    if notes:
        lines += ["", paint("2", clip("  · " + notes[-1], width))]
    if running:
        lines += ["", paint("2", "  Ctrl+C cancels the swarm")]
    return lines

class Screen:
    """Redraws a block of lines in place; prints plain lines when output is not a terminal."""

    def __init__(self):
        self.live = sys.stdout.isatty()
        self.drawn = 0

    def draw(self, lines):
        if not self.live:
            return
        out = (f"\x1b[{self.drawn}F" if self.drawn else "") + "\x1b[J" + "\n".join(lines) + "\n"
        sys.stdout.write(out)
        sys.stdout.flush()
        self.drawn = len(lines)

def watch(swarm):
    """Show a swarm live until it finishes. Ctrl+C cancels it."""
    acts, notes, screen, since = {}, [], Screen(), time.monotonic()

    def listener(message, values):
        worker = values.get("worker")
        if worker is not None:
            text = activity(message)
            if text:
                acts[worker] = text
                if not screen.live:
                    print(f"  agent {worker}: {clean(text)}", flush=True)
        elif message:
            notes.append(message)
            if not screen.live:
                print("  · " + clean(message), flush=True)

    swarm.listener = listener
    frame = 0
    try:
        while swarm.thread.is_alive():
            screen.draw(board(swarm.state, acts, notes, since, frame, shutil.get_terminal_size().columns))
            frame += 1
            swarm.thread.join(0.2)
    except KeyboardInterrupt:
        swarm.cancel.set()
        print(paint("33", "\nCancelling: agents stop after their current step…"), flush=True)
        try:
            swarm.thread.join()
        except KeyboardInterrupt:
            print("Still stopping in the background; local worktrees are preserved.")
            return
    screen.draw(board(swarm.state, acts, notes, since, frame, shutil.get_terminal_size().columns, running=False))
    swarm.listener = None
    summary(swarm)

def summary(swarm):
    s = swarm.state
    phase = s.get("phase")
    print()
    if phase == "ready":
        stat = ""
        with contextlib.suppress(Exception):
            stat = git(swarm.repo, "diff", "--shortstat", s["base"], s["branch"]).stdout.strip()
        tests = s.get("tests") or []
        checks = "no tests configured (add some with /tests)" if not tests else f"{len(tests)} test command(s) passed"
        print(paint("32;1", "✓ Ready for review") + paint("2", f"  {stat} · {checks}"))
        review = [l for l in str(s.get("review", "")).splitlines() if l.strip()][:12]
        if review:
            print(paint("2", "\n  Review:"))
            for line in review:
                print("  " + clip(line, shutil.get_terminal_size().columns - 3))
        print(paint("2", "\n  /apply puts the changes on your branch · /diff shows them · or type a new task"))
    elif phase == "needs_attention":
        print(paint("33;1", "! Needs attention: ") + clean(s.get("error") or "tests failed"))
        failed = [t for t in s.get("tests") or [] if t.get("exit_code")]
        for t in failed[:1]:
            print(paint("2", "  $ " + clean(" ".join(t["command"]))))
            for line in str(t.get("output", "")).splitlines()[-12:]:
                print("    " + clean(line))
        print(paint("2", f"  Review branch {s.get('branch', '')} is kept; /diff shows the changes."))
    elif phase == "cancelled":
        print(paint("33", "⊘ Cancelled. Worktrees are kept under " + str(swarm.folder)))
    else:
        print(paint("31;1", "✗ Failed: ") + clean(s.get("error") or "see the activity above"))

def pair(url, key, name):
    """Connect this PC to a VPS and save the configuration."""
    url = validate_url(url)
    if not key:
        raise CodeError("A Tako client key is required")
    old = load(HOME / "config.json")
    c = {**{k: v for k, v in old.items() if k in {"projects", "engine", "workers", "last_project"}},
         "url": url, "key": key, "name": name, "runner": secrets.token_hex(16)}
    api = Api(c)
    cap = api.request("/v1/code/capabilities")
    if cap.get("protocol") != 1 or not cap.get("free_gateway"):
        raise CodeError("This VPS does not support Tako Code yet; update Tako on the VPS first")
    register(api, c)
    save(HOME / "config.json", c)
    return c

def onboard():
    print("Let's connect this PC to your Tako server. You only do this once.\n")
    while True:
        url = input("  Server URL (https://…): ").strip()
        key = os.environ.get("TAKO_CODE_API_KEY") or getpass.getpass("  Client key (web panel → Access & keys): ").strip()
        try:
            c = pair(url, key, socket.gethostname())
            print(paint("32", "  ✓ Connected.\n"))
            return c
        except CodeError as e:
            print(paint("31", "  ✗ " + str(e)) + "\n")

def add_project(c, path, tests=None):
    root = repo_root(path)
    if HOME.resolve().is_relative_to(root):
        raise CodeError("Keep TAKO_CODE_HOME outside the registered repository")
    pid = hashlib.sha256(str(root).encode()).hexdigest()[:16]
    known = c.setdefault("projects", {}).get(pid, {})
    c["projects"][pid] = {"name": known.get("name", root.name), "path": str(root),
                          "test_commands": tests if tests is not None else known.get("test_commands", [])}
    c["last_project"] = pid
    save(HOME / "config.json", c)
    return pid

def pick_project(c):
    """Use the Git repository in the current directory, or the last one used."""
    try:
        return add_project(c, Path.cwd())
    except CodeError:
        pid = c.get("last_project")
        return pid if pid in c.get("projects", {}) else None

HELP = """  Type what you want built and press Enter. Commands:
    /agents N        how many agents run in parallel (1–6)
    /engine NAME     claude, codex, opencode or builtin
    /tests CMD …     command that checks the result, e.g. /tests npm test  (/tests off)
    /project PATH    work in another Git repository
    /apply           put the last ready swarm's changes on your branch
    /diff            show the last swarm's changes
    /status          show connection, project and settings
    /quit            leave (Ctrl+D also works)"""

def show_diff(job_id):
    patch = HOME / "jobs" / job_id / "changes.patch"
    if not patch.exists():
        print("  No changes recorded for this swarm.")
        return
    for line in clean(patch.read_text(encoding="utf-8", errors="replace"), multiline=True).splitlines():
        color = "32" if line.startswith("+") and not line.startswith("+++") else "31" if line.startswith("-") and not line.startswith("---") else "36" if line.startswith("@@") else "1" if line.startswith("diff ") else ""
        print(paint(color, line) if color else line)

def status_line(c, pid, engine, workers):
    project = c.get("projects", {}).get(pid)
    where = f"{project['name']} ({project['path']})" if project else "no project: cd into a Git repository or use /project PATH"
    return f"  {where}\n  {workers} × {ENGINE_NAMES.get(engine, engine)} · free models via {c['url']}"

def tui():
    enable_terminal()
    print(paint("1", "tako code") + paint("2", f"  {VERSION} · a swarm of coding agents on your PC\n"))
    c = load(HOME / "config.json")
    if not c.get("url") or not c.get("key"):
        c = onboard()
    with runner_lock():
        runner = Runner(c)
        print(paint("2", "  Connecting…"), end="\r", flush=True)
        runner.connect()
        engines = available_engines()
        engine = c.get("engine") if c.get("engine") in engines else next(e for e in ENGINE_ORDER if e in engines)
        workers = c.get("workers", 3)
        pid = pick_project(c)
        print(status_line(c, pid, engine, workers))
        if "claude" not in engines:
            print(paint("2", "  Claude Code isn't installed, so agents use " + ENGINE_NAMES[engine] + ". Install it from https://claude.com/claude-code to use Claude Code agents."))
        print(paint("2", "  Type a task, or /help.\n"))
        runner.on_job = lambda swarm: print(paint("36", f"\n● Swarm started from the web panel: {clip(swarm.job['task'], 60)}. Press Enter to watch."), flush=True)
        threading.Thread(target=runner.background, daemon=True).start()
        last = None
        try:
            while True:
                try:
                    line = input(paint("34;1", "tako ❯ ")).strip()
                except EOFError:
                    break
                except KeyboardInterrupt:
                    print()
                    break
                if not line:
                    if runner.active and runner.active.thread.is_alive():
                        last = runner.active
                        watch(last)
                    continue
                if line.startswith("/"):
                    cmd, _, arg = line.partition(" ")
                    arg = arg.strip()
                    try:
                        if cmd in ("/quit", "/exit", "/q"):
                            break
                        elif cmd == "/help":
                            print(HELP)
                        elif cmd == "/agents":
                            if not arg.isdigit() or not 1 <= int(arg) <= 6:
                                raise CodeError("Choose 1–6 agents, e.g. /agents 4")
                            workers = c["workers"] = int(arg)
                            save(HOME / "config.json", c)
                            print(f"  {workers} agents per swarm.")
                        elif cmd == "/engine":
                            if arg not in ENGINE_NAMES:
                                raise CodeError("Use /engine claude, codex, opencode or builtin")
                            if arg not in available_engines():
                                raise CodeError(f"{ENGINE_NAMES[arg]} isn't installed on this PC")
                            engine = c["engine"] = arg
                            save(HOME / "config.json", c)
                            print(f"  Agents now use {ENGINE_NAMES[engine]}.")
                        elif cmd == "/project":
                            pid = add_project(c, Path(arg or ".").expanduser())
                            print(status_line(c, pid, engine, workers))
                        elif cmd == "/tests":
                            if not pid:
                                raise CodeError("Choose a project first")
                            tests = [] if arg in ("", "off", "none") else [shlex.split(arg, posix=os.name != "nt")]
                            add_project(c, c["projects"][pid]["path"], tests)
                            print("  Tests: " + (" ".join(tests[0]) if tests else "none") + ". They run on the merged result before review.")
                        elif cmd == "/status":
                            print(status_line(c, pid, engine, workers))
                            print("  " + ("connected" if not runner.error else "connection problem: " + runner.error))
                        elif cmd == "/diff":
                            if not last:
                                raise CodeError("No swarm yet")
                            show_diff(last.job["id"])
                        elif cmd == "/apply":
                            if not last:
                                raise CodeError("No swarm yet")
                            apply(last.job["id"])
                            last.state["phase"] = "applied"
                            print(paint("32", "  ✓ The changes are now on your branch (fast-forward). Nothing was pushed."))
                        else:
                            print("  Unknown command. Type /help.")
                    except CodeError as e:
                        print(paint("31", "  " + clean(e)))
                    continue
                # Anything else is a task for the swarm.
                try:
                    if not pid:
                        raise CodeError("cd into a Git repository first, or use /project PATH")
                    repo = Path(c["projects"][pid]["path"])
                    if git(repo, "status", "--porcelain").stdout.strip():
                        raise CodeError("Your repository has uncommitted changes. Commit or stash them first; agents start from your last commit.")
                    last = runner.submit(pid, engine, workers, line)
                except CodeError as e:
                    print(paint("31", "  " + clean(e)))
                    continue
                watch(last)
        finally:
            if runner.active and runner.active.thread.is_alive():
                print("Stopping the running swarm; worktrees are preserved…")
            runner.shutdown()

def main(argv=None):
    parser = argparse.ArgumentParser(prog="tako-code", description="Local coding swarms, free-model routing on your Tako VPS. No code executes on the VPS.")
    sub = parser.add_subparsers(dest="command", required=True, title="commands (run with no command for the interactive swarm UI)")
    setup = sub.add_parser("setup", help="Pair this PC with your VPS using a Tako client key")
    setup.add_argument("--url")
    setup.add_argument("--name", default=socket.gethostname())
    project = sub.add_parser("project", help="Explicitly allow a local repository")
    ps = project.add_subparsers(dest="project_command", required=True)
    add = ps.add_parser("add")
    add.add_argument("path", nargs="?", default=".")
    add.add_argument("--name")
    add.add_argument("--test-json", help='Locally approved test argv, for example ["python3","-m","unittest"]; repeatable', action="append", default=[])
    ps.add_parser("list")
    work = sub.add_parser("run", help="Connect outward and wait for jobs from the web UI")
    work.add_argument("--once", action="store_true", help="Poll once; if assigned a job, run it then exit")
    sub.add_parser("status")
    ap = sub.add_parser("apply", help="Apply a reviewed job to an unchanged local branch")
    ap.add_argument("job")
    sub.add_parser("version")
    argv = sys.argv[1:] if argv is None else argv
    if not argv:
        return tui()
    args = parser.parse_args(argv)
    if args.command == "version":
        print("tako-code " + VERSION)
        return
    if args.command == "setup":
        url = validate_url(args.url or input("Tako VPS URL (https://…): "))
        key = os.environ.get("TAKO_CODE_API_KEY") or getpass.getpass("Tako client key (Access & keys on the VPS): ")
        pair(url, key, args.name)
        print("PC paired. Next: cd into a Git repository and run tako-code")
        return
    c = config()
    if args.command == "project":
        if args.project_command == "list":
            for id, p in c.get("projects", {}).items():
                print(id, p["name"], p["path"])
            return
        root = repo_root(args.path)
        if HOME.resolve().is_relative_to(root):
            raise CodeError("Keep TAKO_CODE_HOME outside the registered repository")
        commands = []
        for raw in args.test_json:
            try:
                command = json.loads(raw)
            except ValueError:
                raise CodeError("Tests must be JSON argv arrays, not shell commands") from None
            if not isinstance(command, list) or not command or not all(isinstance(x, str) and x for x in command):
                raise CodeError("Tests must be nonempty JSON argv arrays")
            commands.append(command)
            if len(commands) > 8:
                raise CodeError("Configure at most eight test commands per project")
        id = hashlib.sha256(str(root).encode()).hexdigest()[:16]
        c.setdefault("projects", {})[id] = {"name": args.name or root.name, "path": str(root), "test_commands": commands}
        save(HOME / "config.json", c)
        print("Registered " + root.name + ". Tests are executed only from this locally configured argv list.")
        return
    if args.command == "status":
        print("VPS:", c["url"], "\nPC:", c["name"], "\nProjects:", len(c.get("projects", {})), "\nEngines:", ", ".join(available_engines()))
        return
    if args.command == "run":
        with runner_lock():
            run(c, args.once)
        return
    if args.command == "apply":
        apply(args.job)

if __name__ == "__main__":
    try:
        main()
    except (CodeError, OSError, subprocess.SubprocessError) as error:
        print("tako-code: " + clean(error), file=sys.stderr)
        sys.exit(1)
