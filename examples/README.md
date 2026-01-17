# FetchKit Examples

This folder contains examples demonstrating how to use FetchKit with various tools and frameworks.

## Prerequisites

Build FetchKit first:

```bash
cargo build -p fetchkit-cli --release
```

## Examples

### Simple MCP Client (`simple_mcp_client.py`)

A minimal example showing direct JSON-RPC communication with the FetchKit MCP server. No external dependencies required.

```bash
# Run with Python directly
python examples/simple_mcp_client.py
```

### LangChain Agent (`langchain_summarize.py`)

A LangChain agent that uses FetchKit via MCP to fetch and summarize web content.

**Requirements:**
- `OPENAI_API_KEY` environment variable

```bash
# Run with uvx (recommended - handles dependencies automatically)
uvx examples/langchain_summarize.py

# Or install dependencies and run
pip install langchain langchain-openai langchain-mcp-adapters langgraph
python examples/langchain_summarize.py
```

This example:
1. Connects to FetchKit MCP server
2. Creates a ReAct agent with the fetchkit tool
3. Fetches https://everruns.com/ as markdown
4. Summarizes the content using GPT-4o-mini

## MCP Tool Reference

The `fetchkit` MCP tool accepts these parameters:

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `url` | string | Yes | URL to fetch (http:// or https://) |
| `method` | string | No | `GET` (default) or `HEAD` |
| `as_markdown` | boolean | No | Convert HTML to Markdown |
| `as_text` | boolean | No | Convert HTML to plain text |

Example tool call:

```json
{
  "name": "fetchkit",
  "arguments": {
    "url": "https://example.com",
    "as_markdown": true
  }
}
```
