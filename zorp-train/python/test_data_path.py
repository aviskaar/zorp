"""Self-check for the corpus path in mlx_train.py.

Runs without MLX and without `tokenizers`, which is the point: what is
checked here is the branch that decides whether a run trains on a real
corpus or on synthetic tokens, and getting that wrong is invisible in a
loss curve.

    python3 zorp-train/python/test_data_path.py
"""
import importlib.util
import json
import os
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))


def load():
    spec = importlib.util.spec_from_file_location("mlx_train", os.path.join(HERE, "mlx_train.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FakeTokenizer:
    """Encodes each word as its length, so ids are predictable."""

    def __init__(self, cap=1000):
        self.cap = cap

    def encode(self, text):
        class R:
            pass

        r = R()
        r.ids = [min(len(w), self.cap) for w in text.split()]
        return r


def test_iter_texts_reads_the_jsonl_shapes():
    m = load()
    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, "c.jsonl")
        with open(p, "w") as f:
            f.write(json.dumps({"text": "from text"}) + "\n")
            f.write(json.dumps({"content": "from content"}) + "\n")
            f.write(json.dumps({"body": "from body"}) + "\n")
            f.write(json.dumps("a bare string") + "\n")
            f.write("\n")                      # blank lines are skipped
            f.write("not json at all\n")       # falls back to the raw line
        assert list(m.iter_texts(p)) == [
            "from text", "from content", "from body",
            "a bare string", "not json at all",
        ]


def test_no_corpus_or_no_tokenizer_means_synthetic():
    m = load()
    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, "c.jsonl")
        with open(p, "w") as f:
            f.write(json.dumps({"text": "one two three"}) + "\n")
        # No tokenizer: synthetic, whatever the corpus says.
        assert m.load_token_stream(p, None, 1000) is None
        # No corpus path at all: synthetic.
        assert m.load_token_stream(None, FakeTokenizer(), 1000) is None
        # A path that is not there: synthetic, not a crash.
        assert m.load_token_stream(os.path.join(d, "nope.jsonl"), FakeTokenizer(), 1000) is None
        # An empty corpus is synthetic too, not a zero length batch later.
        empty = os.path.join(d, "empty.jsonl")
        open(empty, "w").close()
        assert m.load_token_stream(empty, FakeTokenizer(), 1000) is None


def test_corpus_is_encoded_and_out_of_range_ids_are_dropped():
    m = load()
    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, "c.jsonl")
        with open(p, "w") as f:
            f.write(json.dumps({"text": "aa bbbb aa"}) + "\n")
            f.write(json.dumps({"text": "cccccc"}) + "\n")
        ids, dropped = m.load_token_stream(p, FakeTokenizer(), 1000)
        assert ids == [2, 4, 2, 6], ids
        assert dropped == 0
        # A vocabulary smaller than the tokenizer's ids drops, never clamps:
        # clamping would train on a token the text did not contain.
        ids, dropped = m.load_token_stream(p, FakeTokenizer(), 5)
        assert ids == [2, 4, 2], ids
        assert dropped == 1, dropped


def test_train_refuses_without_mlx():
    m = load()
    assert m.HAVE_MLX is False
    # The stand-in must not quietly behave like a tensor library.
    try:
        m.nn.Module()
    except RuntimeError:
        pass
    else:
        raise AssertionError("the mlx stand-in constructed a Module")


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)
    print("all data path checks passed")
