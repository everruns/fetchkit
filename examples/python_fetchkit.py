#!/usr/bin/env python3
"""
Fetchkit Python bindings example.

Demonstrates the fetchkit_py module: fetching URLs, accessing metadata,
and verifying tool contract (schemas, descriptions).

Usage:
    # After building with maturin:
    maturin develop -m crates/fetchkit-python/Cargo.toml
    python examples/python_fetchkit.py
"""

import sys


def main():
    try:
        from fetchkit_py import FetchkitTool, FetchRequest, fetch
    except ImportError:
        print("Error: fetchkit_py module not found.")
        print("Build it with: maturin develop -m crates/fetchkit-python/Cargo.toml")
        sys.exit(1)

    print("Fetchkit Python Example")
    print("========================\n")

    passed = 0
    failed = 0

    # 1. Tool metadata
    print("1. Tool metadata")
    tool = FetchkitTool()
    desc = tool.description()
    llmtxt = tool.llmtxt()
    schema = tool.input_schema()
    assert len(desc) > 0, "Description should not be empty"
    assert len(llmtxt) > 0, "LLM text should not be empty"
    assert "url" in schema, "Schema should contain 'url'"
    print(f"   Description length: {len(desc)}")
    print(f"   LLM text length: {len(llmtxt)}")
    print("   PASS\n")
    passed += 1

    # 2. Fetch HTML as markdown
    print("2. Fetch HTML as markdown")
    resp = tool.fetch("https://example.com", as_markdown=True)
    print(f"   Status: {resp.status_code}")
    print(f"   Format: {resp.format}")
    print(f"   Content length: {len(resp.content or '')}")
    if resp.status_code == 200 and resp.format == "markdown":
        print("   PASS\n")
        passed += 1
    else:
        print("   FAIL\n")
        failed += 1

    # 3. Fetch as plain text
    print("3. Fetch HTML as text")
    resp = tool.fetch("https://example.com", as_text=True)
    print(f"   Status: {resp.status_code}")
    print(f"   Format: {resp.format}")
    if resp.status_code == 200 and resp.format == "text":
        print("   PASS\n")
        passed += 1
    else:
        print("   FAIL\n")
        failed += 1

    # 4. HEAD request
    print("4. HEAD request")
    resp = tool.fetch("https://example.com", method="HEAD")
    print(f"   Status: {resp.status_code}")
    print(f"   Content-Type: {resp.content_type}")
    if resp.status_code == 200 and resp.content is None:
        print("   PASS\n")
        passed += 1
    else:
        print("   FAIL\n")
        failed += 1

    # 5. FetchRequest builder
    print("5. FetchRequest builder")
    req = FetchRequest("https://example.com", as_markdown=True)
    assert req.url == "https://example.com"
    assert req.as_markdown is True
    json_str = req.to_json()
    assert "example.com" in json_str
    req2 = FetchRequest.from_json(json_str)
    assert req2.url == req.url
    print(f"   URL: {req.url}")
    print(f"   JSON: {json_str}")
    print("   PASS\n")
    passed += 1

    # 6. FetchResponse repr
    print("6. FetchResponse repr")
    resp = tool.fetch("https://example.com")
    repr_str = repr(resp)
    assert "FetchResponse" in repr_str
    json_str = resp.to_json()
    assert "status_code" in json_str
    print(f"   repr: {repr_str}")
    print("   PASS\n")
    passed += 1

    # 7. Convenience fetch function
    print("7. Module-level fetch() function")
    resp = fetch("https://example.com", as_markdown=True)
    print(f"   Status: {resp.status_code}")
    if resp.status_code == 200:
        print("   PASS\n")
        passed += 1
    else:
        print("   FAIL\n")
        failed += 1

    # 8. Invalid scheme rejected
    print("8. Invalid scheme rejected")
    try:
        tool.fetch("ftp://example.com")
        print("   Should have raised!\n   FAIL\n")
        failed += 1
    except Exception as e:
        print(f"   Correctly rejected: {e}")
        print("   PASS\n")
        passed += 1

    # 9. Private IP blocked (SSRF protection)
    print("9. Private IP blocked (SSRF protection)")
    try:
        tool.fetch("http://127.0.0.1/")
        print("   Should have raised!\n   FAIL\n")
        failed += 1
    except Exception as e:
        print(f"   Correctly blocked: {e}")
        print("   PASS\n")
        passed += 1

    print("========================")
    print(f"Results: {passed} passed, {failed} failed")

    if failed > 0:
        sys.exit(1)


if __name__ == "__main__":
    main()
