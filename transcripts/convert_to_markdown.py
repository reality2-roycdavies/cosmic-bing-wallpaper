#!/usr/bin/env python3
"""
Convert Claude Code conversation JSONL to readable Markdown.
"""

import json
import re
from pathlib import Path

def extract_text_content(content):
    """Extract text from content blocks."""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        texts = []
        for block in content:
            if isinstance(block, dict):
                if block.get('type') == 'text':
                    texts.append(block.get('text', ''))
                elif block.get('type') == 'tool_use':
                    tool_name = block.get('name', 'unknown')
                    tool_input = block.get('input', {})
                    if tool_name == 'Bash':
                        cmd = tool_input.get('command', '')
                        desc = tool_input.get('description', '')
                        if desc:
                            texts.append(f"\n**[Bash: {desc}]**\n```bash\n{cmd}\n```\n")
                        else:
                            texts.append(f"\n```bash\n{cmd}\n```\n")
                    elif tool_name == 'Write':
                        file_path = tool_input.get('file_path', '')
                        texts.append(f"\n**[Write: {file_path}]**\n")
                    elif tool_name == 'Edit':
                        file_path = tool_input.get('file_path', '')
                        texts.append(f"\n**[Edit: {file_path}]**\n")
                    elif tool_name == 'Read':
                        file_path = tool_input.get('file_path', '')
                        texts.append(f"\n**[Read: {file_path}]**\n")
                    elif tool_name == 'Grep':
                        pattern = tool_input.get('pattern', '')
                        texts.append(f"\n**[Grep: `{pattern}`]**\n")
                    elif tool_name == 'Glob':
                        pattern = tool_input.get('pattern', '')
                        texts.append(f"\n**[Glob: `{pattern}`]**\n")
                    else:
                        texts.append(f"\n**[Tool: {tool_name}]**\n")
            elif isinstance(block, str):
                texts.append(block)
        return ''.join(texts)
    return str(content)

def clean_text(text):
    """Remove system reminders and clean up text."""
    # Remove system-reminder blocks
    text = re.sub(r'<system-reminder>.*?</system-reminder>', '', text, flags=re.DOTALL)
    # Remove function_results blocks (tool outputs)
    text = re.sub(r'<function_results>.*?</function_results>', '*[Tool output received]*', text, flags=re.DOTALL)
    return text.strip()

def convert_jsonl_to_markdown(jsonl_path, output_path):
    """Convert JSONL conversation to Markdown."""

    entries = []
    with open(jsonl_path, 'r') as f:
        for line in f:
            if line.strip():
                try:
                    entries.append(json.loads(line))
                except json.JSONDecodeError:
                    continue

    md_lines = [
        "# Development Conversation Transcript",
        "",
        "This is a transcript of the conversation between Dr. Roy C. Davies and Claude",
        "during the development of cosmic-bing-wallpaper.",
        "",
        "**Note:** Tool outputs have been summarized for readability. See the `.jsonl` file",
        "for the complete raw data including all tool inputs and outputs.",
        "",
        "---",
        ""
    ]

    message_count = 0
    last_speaker = None  # Track who spoke last to avoid repeated headers

    for entry in entries:
        entry_type = entry.get('type', '')

        # Handle user messages
        if entry_type == 'user':
            msg = entry.get('message', {})
            content = msg.get('content', '')
            text = extract_text_content(content)
            text = clean_text(text)
            if text and not text.startswith('*[Tool output'):
                # Only add header if speaker changed
                if last_speaker != 'human':
                    md_lines.append("## Human")
                    md_lines.append("")
                    last_speaker = 'human'
                md_lines.append(text)
                md_lines.append("")
                message_count += 1

        # Handle assistant messages
        elif entry_type == 'assistant':
            msg = entry.get('message', {})
            content = msg.get('content', '')
            text = extract_text_content(content)
            text = clean_text(text)
            if text:
                # Only add header if speaker changed
                if last_speaker != 'claude':
                    md_lines.append("## Claude")
                    md_lines.append("")
                    last_speaker = 'claude'
                md_lines.append(text)
                md_lines.append("")
                message_count += 1

    with open(output_path, 'w') as f:
        f.write('\n'.join(md_lines))

    print(f"Converted {message_count} messages to {output_path}")

if __name__ == '__main__':
    script_dir = Path(__file__).parent

    # Convert Part 1: Initial Creation
    part1_jsonl = script_dir / 'conversation-part1-creation.jsonl'
    part1_md = script_dir / 'CONVERSATION-PART1-CREATION.md'
    if part1_jsonl.exists():
        convert_jsonl_to_markdown(part1_jsonl, part1_md)

    # Convert Part 2: Refinement
    part2_jsonl = script_dir / 'conversation-part2-refinement.jsonl'
    part2_md = script_dir / 'CONVERSATION-PART2-REFINEMENT.md'
    if part2_jsonl.exists():
        convert_jsonl_to_markdown(part2_jsonl, part2_md)
