# Finder may leave Homebrew and Python.org out of PATH.
cc_path_python3=$(command -v python3 2>/dev/null)
cc_found=false
for cc_version in 3.15 3.12; do
    cc_path_versioned=$(command -v "python$cc_version" 2>/dev/null)
    for cc_python in \
        "/opt/homebrew/bin/python$cc_version" \
        "/opt/homebrew/opt/python@$cc_version/bin/python$cc_version" \
        "/usr/local/bin/python$cc_version" \
        "/Library/Frameworks/Python.framework/Versions/$cc_version/bin/python$cc_version" \
        "$cc_path_versioned" /opt/homebrew/bin/python3 /usr/local/bin/python3 "$cc_path_python3"; do
        case "$cc_python" in /*) ;; *) continue ;; esac
        [ -f "$cc_python" ] && [ -x "$cc_python" ] || continue
        # Apple's system shim may request Command Line Tools when invoked.
        [ "$cc_python" != /usr/bin/python3 ] || continue
        cc_found=true
        if "$cc_python" -I -c 'from pathlib import Path
import platform, sys, sysconfig
version = f"{sys.version_info.major}.{sys.version_info.minor}"
base = Path(sys.base_prefix).resolve()
supported_installation = (
    base.is_relative_to(f"/opt/homebrew/Cellar/python@{version}")
    or base.is_relative_to(f"/usr/local/Cellar/python@{version}")
    or base == Path(f"/Library/Frameworks/Python.framework/Versions/{version}"))
sys.exit(not (sys.platform == "darwin" and platform.machine() == "arm64"
    and sys.implementation.name == "cpython" and sys.version_info.releaselevel == "final"
    and not sysconfig.get_config_var("Py_GIL_DISABLED") and version == sys.argv[1]
    and supported_installation))' \
            "$cc_version" </dev/null >/dev/null 2>&1; then
            exec "$cc_python" "$@"
        fi
    done
done
IFS= read -r cc_setup_request
if [ "$cc_found" = true ]; then
    printf '%s\n' '{"ok":false,"error":"setup_macos_python_unsupported"}'
else
    printf '%s\n' '{"ok":false,"error":"setup_macos_python_missing"}'
fi
exit 69
