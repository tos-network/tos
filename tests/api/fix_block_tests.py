#!/usr/bin/env python3
"""
Script to fix block and GHOSTDAG tests to use flat block structure
"""

import re

def fix_block_structure_references(content):
    """
    Replace nested block.header.field with flat block.field
    """
    # Pattern 1: block["header"]["field"]
    content = re.sub(r'block\["header"\]\["(\w+)"\]', r'block["\1"]', content)

    # Pattern 2: block.header["field"]
    content = re.sub(r'block\.header\["(\w+)"\]', r'block["\1"]', content)

    # Pattern 3: header = block["header"]
    content = re.sub(r'header = block\["header"\]', 'header = block  # Block structure is flat', content)

    # Pattern 4: "header" in block assertions
    content = re.sub(r'assert "header" in block', 'assert "hash" in block  # Block structure is flat', content)

    # Pattern 5: block["transactions"] accesses (keep as is, it's at top level)

    return content

def main():
    import sys
    if len(sys.argv) != 2:
        print("Usage: fix_block_tests.py <test_file>")
        sys.exit(1)

    filepath = sys.argv[1]

    with open(filepath, 'r') as f:
        content = f.read()

    fixed_content = fix_block_structure_references(content)

    with open(filepath, 'w') as f:
        f.write(fixed_content)

    print(f"Fixed {filepath}")

if __name__ == "__main__":
    main()
