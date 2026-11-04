"""Guest driver for CPython 3.12 official tests on Slop.

Runs each test module listed in /mntpy/runtests.txt in a separate
subprocess (crash/timeout isolation), classifies the outcome, and prints
structured markers to stdout (captured via the serial log on the host).

Markers:
  PYOFF_PROV <key>=<value> ...   provenance (version, paths)
  PYOFF_BEGIN <name>              per-test start
  PYOFF_LOG <name> <line>         trailing output of failed tests (host parses)
  PYOFF_RESULT <name> <status> <secs> [detail]
                                  status in PASS/FAIL/CRASH/TIMEOUT/SKIP/
                                  RESDENIED, detail is short machine-readable
                                  extra info (e.g. skipped=3, rc=1, sig=-11)
  PYOFF_DONE                      end of run

No Slop-specific hacks: plain stdlib only (subprocess, sys, time, os, re).
"""
import os
import re
import subprocess
import sys
import time

RUNLIST = "/mntpy/runtests.txt"
# Per-test timeout (guest seconds). The harness writes the value to
# /mntpy/timeout.txt; the guest cannot receive it via environ because
# pyrun uses a fixed envp.
def _load_timeout():
    try:
        with open("/mntpy/timeout.txt") as f:
            return int(f.read().strip())
    except (OSError, ValueError):
        return 120
PER_TEST_TIMEOUT = _load_timeout()
# SEEDMODE ram: copy the stdlib/test seed from disk to /tmp (tmpfs) once,
# then run test children from RAM. Decouples test execution from host disk
# latency variance; disk-FS coverage still uses SEEDMODE disk (default).
# The mode is recorded in every result line's provenance.
SEEDMODE = "disk"
RAMLIB = "/tmp/ramlib"
RAMREADY = "/tmp/ramlib.ready"
CHILD_ENV = None


def ensure_no_shadow():
    # The driver lives at /mntpy/test.py, which would shadow the stdlib
    # `test` package for child `python -m test.<name>` runs (cwd precedes
    # PYTHONPATH in sys.path). Run children from / so `test` resolves to
    # /mntpy/lib/test via PYTHONPATH.
    try:
        os.chdir("/")
    except OSError:
        pass


def seed_env():
    """Return the child environment, preparing the RAM seed if requested."""
    global SEEDMODE
    try:
        with open("/mntpy/seedmode.txt") as f:
            SEEDMODE = f.read().strip() or "disk"
    except OSError:
        SEEDMODE = "disk"
    print("PYOFF_SEEDMODE %s" % SEEDMODE, flush=True)
    if SEEDMODE != "ram":
        return None
    import shutil
    if not os.path.exists(RAMREADY):
        print("PYOFF_RAMCOPY start", flush=True)
        t0 = time.monotonic()
        if os.path.exists(RAMLIB):
            shutil.rmtree(RAMLIB)
        shutil.copytree("/mntpy/lib", RAMLIB, symlinks=True)
        with open(RAMREADY, "w") as f:
            f.write("ok\n")
        print("PYOFF_RAMCOPY done %.1f" % (time.monotonic() - t0), flush=True)
    env = dict(os.environ)
    env["PYTHONPATH"] = RAMLIB
    return env


def prov():
    info = []
    info.append("version=%s" % sys.version.replace("\n", " "))
    info.append("executable=%s" % sys.executable)
    info.append("prefix=%s" % sys.prefix)
    info.append("path=%s" % os.pathsep.join(sys.path))
    print("PYOFF_PROV " + " ".join(info), flush=True)
    try:
        import platform
        print("PYOFF_PROV platform=%s libc=%s" % (platform.platform(), "?"), flush=True)
    except Exception as e:
        print("PYOFF_PROV platform=unknown err=%r" % (e,), flush=True)


def classify(name, rc, secs, out):
    """Return (status, detail)."""
    if rc is not None and rc < 0:
        return ("CRASH", "sig=%d" % rc)
    tail = out[-4000:]
    ran = re.findall(r"^Ran (\d+) tests?", tail, re.M)
    skipped = re.findall(r"skipped=(\d+)", tail)
    nskip = sum(int(x) for x in skipped)
    nran = int(ran[-1]) if ran else -1
    if "ResourceDenied" in tail or "resource denied" in tail.lower():
        return ("RESDENIED", "rc=%d ran=%d skipped=%d" % (rc, nran, nskip))
    if rc == 0:
        if nran == 0 or ("NO TESTS RAN" in tail):
            return ("SKIP", "ran=0")
        if nran > 0 and nskip >= nran and re.search(r"^OK", tail, re.M):
            return ("SKIP", "all-skipped=%d" % nskip)
        return ("PASS", "ran=%d skipped=%d" % (nran, nskip))
    # rc != 0: unittest FAILED summary or hard error
    m = re.search(r"^FAILED \((.*)\)", tail, re.M)
    detail = m.group(1) if m else ("rc=%d" % rc)
    if nran == 0 and "skipped" in tail.lower():
        return ("SKIP", "ran=0 " + detail)
    return ("FAIL", detail)


def run_one(name):
    print("PYOFF_BEGIN %s" % name, flush=True)
    t0 = time.monotonic()
    # runone.py installs a faulthandler watchdog (100s) that dumps all
    # thread tracebacks and exits a stuck test, so hangs arrive back here
    # as FAIL with a location instead of a silent driver timeout.
    cmd = [sys.executable, "-u", "-S", "/mntpy/runone.py", name]
    deadline = PER_TEST_TIMEOUT
    try:
        p = subprocess.Popen(cmd, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True, cwd="/",
                             env=CHILD_ENV)
        last_beat = t0
        rc = None
        while True:
            now = time.monotonic()
            if now - last_beat >= 30.0:
                print("PYOFF_BEAT %s elapsed=%.1f" % (name, now - t0), flush=True)
                last_beat = now
            prc = p.poll()
            if prc is not None:
                rc = prc
                break
            if now - t0 >= deadline:
                break
            time.sleep(1.0)
        secs = time.monotonic() - t0
        if rc is None:
            print("PYOFF_KILL %s elapsed=%.1f" % (name, secs), flush=True)
            try:
                p.kill()
            except Exception as e:
                print("PYOFF_KILLERR %s %r" % (name, e), flush=True)
            try:
                out, err = p.communicate(timeout=30)
                rc = p.returncode
            except Exception as e:
                print("PYOFF_COMMERR %s %r" % (name, e), flush=True)
                out, err = "", ""
                rc = p.poll()
        else:
            # The main child may be dead while a stray thread (e.g. the
            # runone watchdog) still holds the pipes open, so never wait
            # forever here: fall back to what was captured so far.
            try:
                out, err = p.communicate(timeout=30)
            except Exception as e:
                print("PYOFF_COMMERR2 %s %r" % (name, e), flush=True)
                try:
                    p.kill()
                except Exception:
                    pass
                out, err = "", ""
                try:
                    out, err = p.communicate(timeout=10)
                except Exception:
                    out, err = out or "", err or ""
        out = (out or "") + ("\n---stderr---\n" if err else "") + (err or "")
        if rc is None:
            print("PYOFF_RESULT %s TIMEOUT %.1f timeout=%d" % (name, secs, PER_TEST_TIMEOUT), flush=True)
        else:
            status, detail = classify(name, rc, secs, out)
            print("PYOFF_RESULT %s %s %.1f %s" % (name, status, secs, detail), flush=True)
            if status in ("FAIL", "CRASH", "RESDENIED"):
                for line in out.splitlines()[-25:]:
                    print("PYOFF_LOG %s %s" % (name, line[:300]), flush=True)
    except Exception as e:
        secs = time.monotonic() - t0
        print("PYOFF_RESULT %s HARNESSFAIL %.1f err=%r" % (name, secs, e), flush=True)


def main():
    global CHILD_ENV
    ensure_no_shadow()
    prov()
    CHILD_ENV = seed_env()
    try:
        with open(RUNLIST) as f:
            names = [w.strip() for w in f.read().split() if w.strip() and not w.startswith("#")]
    except OSError as e:
        print("PYOFF_RESULT _harness HARNESSFAIL 0.0 runlist-missing=%r" % (e,), flush=True)
        print("PYOFF_DONE", flush=True)
        return
    print("PYOFF_PLAN count=%d %s" % (len(names), " ".join(names)), flush=True)
    for n in names:
        run_one(n)
    print("PYOFF_DONE", flush=True)


if __name__ == "__main__":
    main()
