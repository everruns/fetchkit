#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "langchain>=1.0.0",
#     "langchain-openai>=0.2.0",
#     "langchain-mcp-adapters>=0.1.0",
# ]
# ///
"""
LangChain agent example using FetchKit MCP server for web fetching.

This example creates a LangChain agent that can fetch web content using the
FetchKit MCP tool and summarize it using an LLM.

Requirements:
    - OPENAI_API_KEY environment variable set
    - FetchKit CLI built: cargo build -p fetchkit-cli --release

Usage:
    uv run examples/langchain_summarize.py
"""

import asyncio
import os
import sys
 
from langchain.agents import create_agent
from langchain_mcp_adapters.client import MultiServerMCPClient
from langchain_openai import ChatOpenAI


async def main():
    # Check for API key
    if not os.environ.get("OPENAI_API_KEY"):
        print("Error: OPENAI_API_KEY environment variable is required")
        print("Set it with: export OPENAI_API_KEY='your-key-here'")
        sys.exit(1)

    # URL to summarize
    url = "https://everruns.com/"

    print("Creating LangChain agent with FetchKit MCP tool...")
    print(f"Target URL: {url}")
    print()

    # Create MCP client connected to FetchKit server
    mcp_client = MultiServerMCPClient(
        {
            "fetchkit": {
                "command": "cargo",
                "args": ["run", "-p", "fetchkit-cli", "--", "mcp"],
                "transport": "stdio",
            }
        }
    )

    # Get tools from MCP server
    tools = await mcp_client.get_tools()
    print(f"Available MCP tools: {[t.name for t in tools]}")
    print()

    # Create LLM and agent
    llm = ChatOpenAI(model="gpt-5-mini", temperature=0)
    agent = create_agent(llm, tools)

    # Run the agent with a summarization task
    prompt = f"""
    Please fetch the content from {url} using the fetchkit tool with as_markdown=true,
    then provide a concise summary of what the website is about.

    Include:
    1. What the company/product does
    2. Key features or offerings
    3. Target audience
    """

    print("Running agent...")
    print("-" * 50)

    result = await agent.ainvoke({"messages": [("human", prompt)]})

    # Print the final response
    for message in result["messages"]:
        if hasattr(message, "content") and message.content:
            if hasattr(message, "type") and message.type == "ai":
                print("\nAgent response:")
                print(message.content)


if __name__ == "__main__":
    asyncio.run(main())
