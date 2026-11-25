# Print only process names visible to the current account for one listening port.
proto=$1
port=$2
export LC_ALL=C
if [ "$(uname -s)" = Linux ] && command -v ss >/dev/null 2>&1; then
  if [ "$proto" = udp ]; then flag=-u; else flag=-t; fi
  ss -H -l -n -p "$flag" "sport = :$port" 2>/dev/null | awk '{
    rest=$0
    while (match(rest, /\("[^"]+",pid=/)) {
      print substr(rest, RSTART+2, RLENGTH-8)
      rest=substr(rest, RSTART+RLENGTH)
    }
  }' | sort -u
elif command -v lsof >/dev/null 2>&1; then
  if [ "$proto" = udp ]; then
    lsof -nP -Fcn -iUDP:"$port" 2>/dev/null | awk -v port="$port" '/^c/{cmd=substr($0,2)} /^n/{split($0,a,"->"); if(a[1] ~ (":" port "$")) print cmd}' | sort -u
  else
    lsof -nP -Fc -a -iTCP:"$port" -sTCP:LISTEN 2>/dev/null | sed -n 's/^c//p' | sort -u
  fi
fi
