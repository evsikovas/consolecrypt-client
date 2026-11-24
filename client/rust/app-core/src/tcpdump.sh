# Invoked through SSH exec in its own PTY. Only tcpdump's header summary
# is returned. The PTY hangs up the foreground process when capture stops.
export LC_ALL=C PATH="$PATH:/usr/sbin:/sbin"
if ! command -v tcpdump >/dev/null 2>&1; then
  for manager in apt-get dnf yum apk pacman zypper pkg brew; do
    if command -v "$manager" >/dev/null 2>&1; then
      echo "CC_MISSING $manager $(id -u)"
      exit 43
    fi
  done
  echo "CC_MISSING unknown"
  exit 43
fi
peer=$1
if [ -z "$peer" ]; then
  echo CC_TCPDUMP_READY
  exit 0
fi
sudo_mode=$2
set -f
set -- $SSH_CONNECTION
if [ "$#" -ne 4 ]; then echo CC_NO_PEER; exit 44; fi
for ip in "$1" "$3"; do
  case "$ip" in ""|*[!a-fA-F0-9:.]*) echo CC_NO_PEER; exit 44;; esac
done
for port in "$2" "$4"; do
  case "$port" in ""|*[!0-9]*) echo CC_NO_PEER; exit 44;; esac
done
exclude="not (tcp and ((src host $1 and src port $2 and dst host $3 and dst port $4) or (src host $3 and src port $4 and dst host $1 and dst port $2)))"
if command -v ip >/dev/null 2>&1; then
  addrs=$(ip -o addr show | awk '$3=="inet" || $3=="inet6" {split($4,a,"/"); print a[1]}' | sort -u)
else
  addrs=$(ifconfig -a | awk '$1=="inet" || $1=="inet6" {split($2,a,"%"); print a[1]}' | sort -u)
fi
sources=""; destinations=""
for addr in $addrs; do
  case "$addr" in ""|*[!a-fA-F0-9:.]*) continue;; esac
  sources="${sources:+$sources or }src host $addr"
  destinations="${destinations:+$destinations or }dst host $addr"
done
if [ -z "$sources" ]; then echo CC_NO_PEER; exit 44; fi
pair="(dst host $peer and ($sources)) or (src host $peer and ($destinations))"
set -- tcpdump -p -nn -tt -l -s 96 -c 2000
if [ "$(uname -s)" = Linux ]; then set -- "$@" -i any; fi
if [ "$sudo_mode" = sudo ] && [ "$(id -u)" != 0 ]; then set -- sudo -n "$@"; fi
exec "$@" "(ip or ip6) and ($pair) and $exclude" 2>&1
