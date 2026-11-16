# Fixed Native preflight. No caller command or interpreter path is accepted.
# Native writes one bounded JSON line before reading stdout. Drain that line
# when Python is absent so the fixed diagnostic is not replaced by BrokenPipe.
if [ ! -f /usr/bin/python3 ] || [ ! -x /usr/bin/python3 ]; then
    IFS= read -r cc_setup_request
    printf '%s\n' '{"ok":false,"error":"setup_python_missing"}'
    exit 69
fi
exec /usr/bin/python3 "$@"
