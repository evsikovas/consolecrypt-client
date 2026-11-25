"""Read one explicitly requested web origin without credentials or redirects."""
import base64
import http.client
import json
import signal
import socket
import ssl
import sys


def preview(host, port, https):
    connection = (http.client.HTTPSConnection if https else http.client.HTTPConnection)(
        host, port, timeout=4
    )
    try:
        connection.request("GET", "/", headers={"Accept": "text/html, */*;q=0.1", "Accept-Encoding": "identity"})
        response = connection.getresponse()
        mime = response.headers.get_content_type() if response.headers.get("Content-Type") else ""
        charset = response.headers.get_content_charset() or ""
        body = b""
        if mime in ("text/html", "application/xhtml+xml", "") and response.getheader("Content-Encoding", "identity").lower() == "identity":
            for _ in range(32):
                chunk = response.read1(1024)
                body += chunk
                if not chunk or b"</head>" in body.lower():
                    break
        return {"status": response.status, "mime": mime[:100], "charset": charset[:40],
                "body": base64.b64encode(body).decode("ascii")}
    except ssl.SSLCertVerificationError:
        return {"error": "certificate"}
    except (TimeoutError, socket.timeout):
        return {"error": "timeout"}
    except (http.client.HTTPException, OSError, ValueError):
        return {"error": "no_response"}
    finally:
        connection.close()


if __name__ == "__main__":
    def expired(*_):
        raise TimeoutError()

    signal.signal(signal.SIGALRM, expired)
    signal.alarm(6)
    print(json.dumps(preview(sys.argv[1], int(sys.argv[2]), sys.argv[3] == "https")))
