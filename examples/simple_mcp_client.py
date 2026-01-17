#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""
Simple MCP client example for FetchKit.

This example demonstrates how to communicate with the FetchKit MCP server
using raw JSON-RPC over stdio. No external dependencies required.

Usage:
    uvx python examples/simple_mcp_client.py

Or run directly:
    python examples/simple_mcp_client.py
"""

import json
import subprocess
import sys


def send_request(proc: subprocess.Popen, request: dict) -> dict:
    """Send a JSON-RPC request and read the response."""
    request_json = json.dumps(request)
    proc.stdin.write(request_json + "\n")
    proc.stdin.flush()

    response_line = proc.stdout.readline()
    if not response_line:
        raise RuntimeError("No response from MCP server")

    return json.loads(response_line)


def main():
    # Start the MCP server
    # Assumes fetchkit-cli is built: cargo build -p fetchkit-cli --release
    proc = subprocess.Popen(
        ["cargo", "run", "-p", "fetchkit-cli", "--", "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )

    try:
        # 1. Initialize the connection
        init_request = {
            "jsonrpc": "2.0",
            "id": "1",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "simple-client", "version": "1.0.0"},
            },
        }
        init_response = send_request(proc, init_request)
        print("Server initialized:")
        print(json.dumps(init_response.get("result", {}), indent=2))
        print()

        # Send initialized notification
        initialized_notification = {
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        }
        proc.stdin.write(json.dumps(initialized_notification) + "\n")
        proc.stdin.flush()

        # 2. List available tools
        list_tools_request = {
            "jsonrpc": "2.0",
            "id": "2",
            "method": "tools/list",
            "params": {},
        }
        tools_response = send_request(proc, list_tools_request)
        print("Available tools:")
        tools = tools_response.get("result", {}).get("tools", [])
        for tool in tools:
            print(f"  - {tool['name']}: {tool['description']}")
        print()

        # 3. Call the fetchkit tool to fetch a URL as markdown
        fetch_request = {
            "jsonrpc": "2.0",
            "id": "3",
            "method": "tools/call",
            "params": {
                "name": "fetchkit",
                "arguments": {
                    "url": "https://example.com",
                    "as_markdown": True,
                },
            },
        }
        print("Fetching https://example.com as markdown...")
        fetch_response = send_request(proc, fetch_request)

        # Check for errors
        if "error" in fetch_response:
            print(f"Error: {json.dumps(fetch_response['error'], indent=2)}")
        else:
            result = fetch_response.get("result", {})
            content = result.get("content", [])
            if content and content[0].get("text"):
                text = content[0]["text"]
                # Response text is prettified JSON from FetchKit, or error string
                try:
                    response_data = json.loads(text)
                    print(f"Status: {response_data.get('status_code')}")
                    print(f"Format: {response_data.get('format')}")
                    content_text = response_data.get("content", "")
                    if content_text:
                        print(f"Content preview:\n{content_text[:500]}...")
                    elif response_data.get("error"):
                        print(f"Fetch error: {response_data.get('error')}")
                except json.JSONDecodeError:
                    # Error responses may be plain text
                    print(f"Response: {text}")
            else:
                print(f"Raw response: {json.dumps(result, indent=2)}")

    finally:
        proc.terminate()
        proc.wait()


if __name__ == "__main__":
    main()
