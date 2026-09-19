import argparse
import json
import os
from tokenizers import Tokenizer, decoders, models, normalizers, pre_tokenizers, trainers

def train_bpe(dataset_path: str, output_dir: str, vocab_size: int, special_tokens: list):
    tokenizer = Tokenizer(models.BPE(unk_token=None))
    tokenizer.normalizer = normalizers.NFKC()
    tokenizer.pre_tokenizer = pre_tokenizers.ByteLevel(add_prefix_space=False)
    tokenizer.decoder = decoders.ByteLevel()

    trainer = trainers.BpeTrainer(
        vocab_size=vocab_size,
        special_tokens=special_tokens,
        initial_alphabet=pre_tokenizers.ByteLevel.alphabet()
    )

    def text_iterator():
        with open(dataset_path, "r", encoding="utf-8") as f:
            for line in f:
                if not line.strip():
                    continue
                try:
                    row = json.loads(line)
                    if isinstance(row, dict):
                        text = row.get("text") or row.get("content") or row.get("body", "")
                    elif isinstance(row, str):
                        text = row
                    else:
                        text = ""
                    if text:
                        yield text
                except Exception:
                    yield line.strip()

    tokenizer.train_from_iterator(text_iterator(), trainer=trainer)
    os.makedirs(output_dir, exist_ok=True)
    tokenizer.save(os.path.join(output_dir, "tokenizer.json"))
    
    with open(os.path.join(output_dir, "tokenizer_config.json"), "w") as f:
        json.dump({
            "vocab_size": vocab_size,
            "special_tokens": special_tokens
        }, f, indent=2)
    print("TOKENIZER_TRAINED_OK")

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--dataset", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--vocab-size", type=int, default=32768)
    parser.add_argument("--special-tokens", nargs="*", default=["<|endoftext|>", "<|im_start|>", "<|im_end|>", "<|pad|>"])
    args = parser.parse_args()
    train_bpe(args.dataset, args.output, args.vocab_size, args.special_tokens)
