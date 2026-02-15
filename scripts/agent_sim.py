#!/usr/bin/env python3
import argparse
import json
import ssl
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any


class AgentSimHandler(BaseHTTPRequestHandler):
    node = "unknown"

    def do_POST(self):
        if self.path != "/tfpv1/deliver":
            self.send_response(404)
            self.end_headers()
            return

        content_length = int(self.headers.get("Content-Length", "0"))
        raw_body = self.rfile.read(content_length) if content_length > 0 else b"{}"

        payload: dict[str, Any]
        try:
            decoded = json.loads(raw_body.decode("utf-8"))
            if isinstance(decoded, dict):
                payload = decoded
            else:
                payload = {"raw": decoded}
        except json.JSONDecodeError:
            payload = {"invalid_json": raw_body.decode("utf-8", errors="replace")}

        delivery_id = payload.get("delivery_id", "")
        message_raw = payload.get("message", {})
        message = message_raw if isinstance(message_raw, dict) else {}
        print(
            f"[{self.node}] deliver message_id={message.get('message_id')} trace_id={message.get('trace_id')} "
            f"from={message.get('from_ref')} to={message.get('to_ref')} delivery_id={delivery_id}"
        )

        response = {
            "version": "TFPv1",
            "ack": "processed",
            "delivery_id": delivery_id,
        }

        encoded = json.dumps(response).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def log_message(self, format, *args):
        return


def main():
    parser = argparse.ArgumentParser(description="TFPv1 agent simulator")
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=9443)
    parser.add_argument("--node", required=True)
    parser.add_argument("--tls-cert", required=True)
    parser.add_argument("--tls-key", required=True)
    args = parser.parse_args()

    AgentSimHandler.node = args.node
    server = HTTPServer((args.host, args.port), AgentSimHandler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(certfile=args.tls_cert, keyfile=args.tls_key)
    server.socket = context.wrap_socket(server.socket, server_side=True)

    print(f"agent_sim listening on https://{args.host}:{args.port}/tfpv1/deliver as {args.node}")
    server.serve_forever()


if __name__ == "__main__":
    main()
