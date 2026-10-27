# BusyBox applet compatibility battery.
#
# The same static BusyBox binary runs on the host (Linux) and inside the
# guest kernel, so every line printed here can be compared byte for byte.
# Only deterministic output is used: no timestamps, no host paths, no
# process listings and no identity numbers.
#
# Each check prints one line: BB|<name>|<value>

umask 0022
# Fixed alternate workdir keeps host-vs-guest output byte-identical; a
# per-run fallback covers the case where it is not writable (e.g. a stale
# root-owned copy from another session).
W=/tmp/bbwork-slop
rm -rf "$W" 2>/dev/null
mkdir -p "$W" 2>/dev/null
if ! cd "$W" 2>/dev/null; then
  W=/tmp/bbwork-slop-$$
  rm -rf "$W" 2>/dev/null
  mkdir -p "$W" 2>/dev/null
  cd "$W" 2>/dev/null || { echo "BB|setup|cannot-enter-workdir"; exit 1; }
fi

out() { printf 'BB|%s|%s\n' "$1" "$2"; }
flat() { printf '%s' "$1" | tr '\n' '~'; }
have() { command -v "$1" >/dev/null 2>&1 && echo yes || echo no; }

# --- shell builtins -------------------------------------------------------
out shell-echo "hello world"
out shell-printf "$(printf '%s-%d' abc 42)"
out shell-arith "$((2 + 3 * 4))"
out shell-expansion "${undefined_var:-fallback}"
i=0
while [ $i -lt 3 ]; do i=$((i + 1)); done
out shell-while "$i"
for x in a b c; do last=$x; done
out shell-for "$last"
case abc in a*) out shell-case "matched" ;; *) out shell-case "missed" ;; esac
fn() { out shell-func "$1-$2"; }
fn one two
out shell-subst "$(echo sub $(echo nested))"
out shell-printf-fmt "$(printf '%05.2f|%s|%x' 3.14159 abc 255)"
here=$(cat <<'EOF'
heredoc line
EOF
)
out shell-heredoc "$here"
out shell-glob "$(echo tests/*)"
out shell-quotes "'single' \"double\""
out shell-brace "{a,b}c"
out shell-cmd-sub "$(basename /usr/bin/ls)"
out shell-true 0
false
out shell-false-status "$?"
out shell-test "[ -d / ] && echo dir"
out shell-uname-m "$(uname -m)"
out shell-umask "$(umask)"
out shell-type "$(type printf 2>/dev/null | head -1 | cut -d' ' -f1)"
out shell-command-v "$(command -v printf 2>/dev/null)"
out shell-getopts "$(set -- -a -b val; while getopts "ab:" o; do echo "$o$OPTARG"; done | tr '\n' ' ')"
read r1 r2 <<'EOF'
x y z
EOF
out shell-read-vars "$r1-$r2"

# --- filesystem ------------------------------------------------------------
printf 'l1\nl2\nl3\n' >f.txt
out fs-cat "$(flat "$(cat f.txt)")"
out fs-wc-l "$(wc -l <f.txt)"
out fs-wc-c "$(wc -c <f.txt)"
out fs-head "$(flat "$(head -2 f.txt)")"
out fs-tail "$(flat "$(tail -1 f.txt)")"
out fs-sed "$(printf 'hello\n' | sed 's/h/H/')"
out fs-sed-addr "$(printf 'a\nb\nc\n' | sed '2d' | tr '\n' ' ')"
out fs-sed-n "$(printf 'a\nb\nc\n' | sed -n '2p')"
out fs-grep "$(printf 'foo\nbar\n' | grep '^b')"
out fs-grep-c "$(printf 'foo\nbar\nfoo\n' | grep -c foo)"
out fs-grep-q "$(printf 'foo\n' | grep -q foo && echo hit)"
out fs-awk "$(printf '1 2\n3 4\n' | awk '{s += $2} END {print s}')"
out fs-cut "$(printf 'a:b:c\n' | cut -d: -f2)"
out fs-tr "$(printf 'abc\n' | tr 'a-z' 'A-Z')"
out fs-sed-multi "$(printf 'a\nb\n' | sed 's/$/!/' | tr '\n' ' ')"
out fs-sort "$(printf 'b\na\nc\n' | sort | tr '\n' ' ')"
out fs-sort-u "$(printf 'b\na\nb\n' | sort -u | tr '\n' ' ')"
out fs-sort-n "$(printf '10\n9\n100\n' | sort -n | tr '\n' ' ')"
out fs-uniq-c "$(printf 'a\na\nb\n' | uniq -c | tr -s ' ' | tr '\n' ' ')"
out fs-rev "$(printf 'abc\n' | rev)"
out fs-tac "$(printf 'a\nb\n' | tac | tr '\n' ' ')"
out fs-nl-cat "$(printf 'x\n' | nl)"
out fs-seq "$(seq 1 5 | tr '\n' ' ')"
out fs-expr "$(expr 6 \* 7)"
out fs-base64 "$(printf 'slop' | base64)"
out fs-md5 "$(printf 'slop' | md5sum | cut -d' ' -f1)"
out fs-sha256 "$(printf 'slop' | sha256sum | cut -d' ' -f1)"
out fs-cksum "$(printf 'slop' | cksum | cut -d' ' -f1)"
out fs-od "$(printf 'AB' | od -An -c | tr -s ' ' | tr -d '\n')"
out fs-hexdump "$(printf 'AB' | hexdump -C | head -1 | cut -c49-)"
out fs-cmp "$(printf 'x\n' >c1; printf 'x\n' >c2; cmp c1 c2 && echo same)"
out fs-diff "$(printf 'a\nb\n' >d1; printf 'a\nc\n' >d2; diff d1 d2 | tail -1)"
out fs-wc-word "$(printf 'a b c\n' | wc -w)"

# --- file operations ------------------------------------------------------
mkdir -p sub/deep
out fs-mkdir-p "$(test -d sub/deep && echo nested)"
cp f.txt copy.txt
out fs-cp "$(flat "$(cat copy.txt)")"
mv copy.txt moved.txt
out fs-mv "$(test -f moved.txt && echo moved)"
rm moved.txt
out fs-rm "$(test -f moved.txt || echo removed)"
touch empty.txt
out fs-touch "$(test -f empty.txt && echo created)"
ln -s f.txt link.txt
out fs-readlink "$(readlink link.txt)"
out fs-readlink-f "$(readlink -f link.txt | sed "s|^.*/bbwork[^/]*/|BBWORK/|")"
out fs-cat-symlink "$(cat link.txt)"
out fs-basename "$(basename /usr/bin/ls)"
out fs-dirname "$(dirname /usr/bin/ls)"
out fs-realpath "$(realpath . | sed "s|^.*/bbwork[^/]*$|BBWORK|")"
out fs-pwd "$(pwd | sed "s|^.*/bbwork[^/]*$|BBWORK|")"
mkdir -p sub/../sub2
out fs-mkdir-dotdot "$(test -d sub2 && echo ok)"
rmdir sub2
out fs-rmdir "$(test -d sub2 || echo gone)"
out fs-stat-size "$(stat -c %s f.txt)"
out fs-truncate "$(truncate -s 2 f.txt; stat -c %s f.txt)"
out fs-install "$(printf 'inst\n' >src.txt; install -m 644 src.txt inst.txt; stat -c %s inst.txt)"
out fs-find "$(find . -maxdepth 1 -name '*.txt' | sort | tr '\n' ' ')"
out fs-find-type "$(find . -maxdepth 1 -type d | sort | tr '\n' ' ')"
out fs-ls-files "$(ls -1 *.txt | sort | tr '\n' ' ')"
out fs-dup-file "$(cp f.txt f2.txt; cp f.txt f3.txt; cat f2.txt f3.txt | tr '\n' ' ')"
out fs-append "$(printf 'more\n' >>f2.txt; tail -1 f2.txt)"
out fs-tee "$(printf 'tee\n' | tee t1.txt)"
out fs-xargs "$(printf 'a\nb\n' | xargs echo)"
out fs-mktemp "$(mktemp 2>/dev/null | wc -c)"
out fs-dd "$(dd if=/dev/zero of=z.bin bs=4 count=2 2>/dev/null; stat -c %s z.bin)"
out fs-devnull "$(cat /dev/null | wc -c)"
out fs-urandom "$(head -c 1 /dev/urandom 2>/dev/null | wc -c)"
out fs-write-fail "$(printf 'x' >/nonexistent-dir/file 2>/dev/null; echo $?)"
out fs-rm-nonexistent "$(rm /no-such-file 2>/dev/null; echo $?)"

# --- process control ------------------------------------------------------
out proc-kill0 "$(kill -0 $$ 2>/dev/null; echo $?)"
out proc-kill-missing "$(kill -0 999999 2>/dev/null; echo $?)"
out proc-sleep-timeout "$(timeout 1 sleep 5 2>/dev/null; echo $?)"
out proc-yes-head "$(yes | head -1)"
out proc-bg-exit "$( (sleep 0 & wait) 2>/dev/null; echo $? )"
trap 'echo "BB|proc-trap-exit|trapped"' EXIT
out proc-trap-exit "body"
trap 'echo "BB|proc-signal|usr1"' USR1
kill -USR1 $$
out proc-signal "after"
out proc-subshell-exit "$( (exit 3); echo $? )"
out proc-env-empty "$(env -i | wc -l)"

# --- applet presence ------------------------------------------------------
out applet-echo "$(have echo)"
out applet-ls "$(have ls)"
out applet-sed "$(have sed)"
out applet-awk "$(have awk)"
out applet-find "$(have find)"
out applet-xargs "$(have xargs)"
out applet-sha256sum "$(have sha256sum)"
out applet-install "$(have install)"
out applet-truncate "$(have truncate)"
out applet-timeout "$(have timeout)"
out applet-mktemp "$(have mktemp)"
out applet-tar "$(have tar)"
out applet-od "$(have od)"
out applet-hexdump "$(have hexdump)"
out applet-realpath "$(have realpath)"
out applet-df "$(have df)"
out applet-ps "$(have ps)"
out applet-mount "$(have mount)"

echo "BB|done|end"
