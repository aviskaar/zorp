import argparse
import json
import os
import sys
from tokenizers import Tokenizer

def inspect(tokenizer_dir: str, text: str):
    tok_path = os.path.join(tokenizer_dir, "tokenizer.json")
    tok = Tokenizer.from_file(tok_path)
    encoding = tok.encode(text)
    tokens = encoding.tokens
    ids = encoding.ids
    char_count = len(text)
    token_count = len(ids)
    compression = round(char_count / max(token_count, 1), 2)
    
    result = {
        "tokens": tokens,
        "ids": ids,
        "char_count": char_count,
        "token_count": token_count,
        "compression_chars_per_token": compression
    }
    print(json.dumps(result))

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--tokenizer-dir", required=True)
    parser.add_argument("--text", required=True)
    args = parser.parse_args()
    inspect(args.tokenizer_dir, args.text)
