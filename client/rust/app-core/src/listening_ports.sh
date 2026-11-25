# Print unique local socket ports in the requested interval.
set -eu
export LC_ALL=C
proto=$1
low=$2
high=$3
case "$(uname -s)" in
  Linux)
    set --
    for table in "/proc/net/$proto" "/proc/net/${proto}6"; do
      if [ -r "$table" ]; then set -- "$@" "$table"; fi
    done
    [ "$#" -gt 0 ] || exit 1
    awk -v state="$(if [ "$proto" = tcp ]; then echo 0A; else echo 07; fi)" -v low="$low" -v high="$high" '
      $4 == state {
        split($2, a, ":")
        p = 0
        for (i = 1; i <= length(a[2]); i++)
          p = p * 16 + index("0123456789ABCDEF", substr(toupper(a[2]), i, 1)) - 1
        if (p >= low && p <= high && !seen[p]++) print p
      }' "$@"
    ;;
  Darwin|FreeBSD)
    sockets=$(/usr/bin/env netstat -an -p "$proto") || exit 1
    printf '%s\n' "$sockets" | awk -v proto="$proto" -v low="$low" -v high="$high" '
      index($1, proto) == 1 && ((proto == "tcp" && $6 == "LISTEN") || (proto == "udp" && $5 == "*.*")) {
        n = split($4, a, /\./)
        if (a[n] ~ /^[0-9]+$/) {
          p = a[n] + 0
          if (p >= low && p <= high && !seen[p]++) print p
        }
      }'
    ;;
  *) exit 69 ;;
esac
