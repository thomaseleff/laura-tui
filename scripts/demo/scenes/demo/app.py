from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

GREETINGS = {"en": "Hello", "es": "Hola"}


class Hello(BaseHTTPRequestHandler):
    def do_GET(self):
        q = parse_qs(urlparse(self.path).query)
        name, lang = q.get("name", ["world"])[0], q.get("lang", ["en"])[0]
        if lang not in GREETINGS:  # bug: no fallback to English
            self.send_error(400, f"unknown lang: {lang}")
            return
        body = f"{GREETINGS[lang]}, {name}!\n".encode()
        self.send_response(200)
        self.end_headers()
        self.wfile.write(body)


HTTPServer(("127.0.0.1", 8766), Hello).serve_forever()
