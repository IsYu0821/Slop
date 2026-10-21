#!/bin/sh
# Minimal BusyBox shell battery: the part of tests/bbtest.sh that exercises
# the shell builtins around the allocator. Used to reproduce allocator faults
# quickly while the kernel is being debugged.
out() { printf 'BB|%s|%s\n' "$1" "$2"; }

out shell-echo "$(echo hello world)"
out shell-printf "$(printf '%s-%s' abc 42)"
out shell-arith "$(( 2 + 3 * 4 ))"
out shell-expansion "${UNSET_VAR:-fallback}"
i=0
while [ $i -lt 3 ]; do i=$((i + 1)); done
out shell-while "$i"
for v in a b c; do last=$v; done
out shell-for "$last"
case abc in a*) out shell-case matched ;; *) out shell-case no ;; esac
f() { echo "$1-$2"; }
out shell-func "$(f one two)"
out shell-brace "$(echo {a,b}c)"
out shell-cmd-sub "$(ls)"
out shell-true "$(true; echo $?)"
out shell-false-status "$(false; echo $?)"
out shell-test "$( [ -d / ] && echo dir )"
out shell-uname-m "$(uname -m)"
out shell-umask "$(umask)"
out shell-type "$(type printf 2>/dev/null | head -1 | cut -d' ' -f1)"
out shell-command-v "$(command -v printf 2>/dev/null)"
out shell-getopts "$(set -- -a -b val; while getopts "ab:" o; do echo "$o$OPTARG"; done | tr '\n' ' ')"
read r1 r2 <<'EOF'
x y z
EOF
out shell-read-vars "$r1-$r2"
out shell-done "end"
