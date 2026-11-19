# The app launched from Finder may have no Homebrew directories in PATH.
for cc_python in /opt/homebrew/bin/python3.12 /usr/local/bin/python3.12 /Library/Frameworks/Python.framework/Versions/3.12/bin/python3.12; do
    if [ -f "$cc_python" ] && [ -x "$cc_python" ]; then
        exec "$cc_python" "$@"
    fi
done
IFS= read -r cc_setup_request
printf '%s\n' '{"ok":false,"error":"setup_macos_python_missing"}'
exit 69
