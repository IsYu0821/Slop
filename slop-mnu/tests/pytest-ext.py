import sys
sys.path.insert(0, "/mntpy")
print("PYEXT_OK", sys.version.split()[0], flush=True)
print("PYTRY_DOT", flush=True)
import tp.submod as m
print("PYDOT_OK", m.getval(), flush=True)
