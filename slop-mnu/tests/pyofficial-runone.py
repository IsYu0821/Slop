"""Guest per-test runner: executes one official test module with a
faulthandler watchdog so hangs become located failures (traceback on
stderr) instead of silent stalls. Invoked as:
    /bin/python3 -u -S /mntpy/runone.py <testname>
The watchdog (100s guest time) dumps tracebacks of all threads and exits
if the test does not finish; the harness driver captures the output.
"""
import sys


def main():
    # sys.path[0] is /mntpy (this script's directory), which contains
    # test.py (the harness driver). That module would shadow the stdlib
    # `test` package for the runpy import below, so drop it: the package
    # resolves via PYTHONPATH instead.
    sys.path = [p for p in sys.path if p != "/mntpy"]
    name = sys.argv[1]
    # unittest.main() consumes sys.argv[1:] as test names; the module name
    # we were given is not a test selector, so drop it.
    sys.argv = sys.argv[:1]
    import faulthandler
    import runpy
    faulthandler.dump_traceback_later(100, exit=True)
    try:
        runpy.run_module("test." + name, run_name="__main__", alter_sys=True)
    finally:
        faulthandler.cancel_dump_traceback_later()


main()
