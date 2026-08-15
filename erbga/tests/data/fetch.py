"""Fetch the four ERBGA benchmark networks and write them as plain edge lists.

Counts are verified against Table 2 of the ERBGA thesis before anything is
written, so a moved or changed upstream file fails loudly instead of
silently producing a different benchmark.

These are other people's datasets, redistributed in this repo. Each output
file carries its attribution in its own header, written from CREDITS below,
so regenerating never silently drops it. NOTICE.md carries the same list.
"""

import io
import os
import urllib.request
import zipfile

import networkx as nx

OUT = "erbga/tests/data"

# (name, url or None for a networkx builtin, expected_nodes, expected_edges)
SPECS = [
    ("karate", None, 34, 78),
    ("dolphins", "http://websites.umich.edu/~mejn/netdata/dolphins.zip", 62, 159),
    ("polbooks", "http://websites.umich.edu/~mejn/netdata/polbooks.zip", 105, 441),
    ("football", "http://websites.umich.edu/~mejn/netdata/football.zip", 115, 613),
]

# Attribution, written into each output file. The three fetched from
# Newman's collection ask that the original work be cited, not the
# collection. Keep in sync with NOTICE.md.
CREDITS = {
    "karate": [
        "Zachary's karate club, from the copy bundled with networkx.",
        'W. W. Zachary, "An information flow model for conflict and fission',
        'in small groups", Journal of Anthropological Research 33, 452-473',
        "(1977).",
    ],
    "dolphins": [
        "The Doubtful Sound dolphin social network, via M. E. J. Newman's",
        "network data collection. D. Lusseau, K. Schneider, O. J. Boisseau,",
        "P. Haase, E. Slooten, and S. M. Dawson, Behavioral Ecology and",
        "Sociobiology 54, 396-405 (2003).",
    ],
    "polbooks": [
        "Books about US politics, via M. E. J. Newman's network data",
        "collection. Compiled by V. Krebs, unpublished.",
    ],
    "football": [
        "US college football games, Fall 2000, via M. E. J. Newman's network",
        'data collection. M. Girvan and M. E. J. Newman, "Community structure',
        'in social and biological networks", PNAS 99, 7821-7826 (2002).',
    ],
}


def load(url):
    if url is None:
        return nx.karate_club_graph()
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    raw = urllib.request.urlopen(req, timeout=60).read()
    zf = zipfile.ZipFile(io.BytesIO(raw))
    gml = [n for n in zf.namelist() if n.endswith(".gml")][0]
    return nx.parse_gml(zf.read(gml).decode("latin-1"), label=None)


def main():
    os.makedirs(OUT, exist_ok=True)
    ok = True
    for name, url, want_n, want_m in SPECS:
        g = nx.Graph(load(url))
        g.remove_edges_from(nx.selfloop_edges(g))
        nodes = sorted(g.nodes(), key=str)
        idx = {node: i for i, node in enumerate(nodes)}
        n, m = g.number_of_nodes(), g.number_of_edges()
        good = n == want_n and m == want_m
        ok = ok and good
        print(
            f"{'OK      ' if good else 'MISMATCH'} {name:9s} "
            f"nodes={n:4d} (want {want_n:4d})  edges={m:4d} (want {want_m:4d})  "
            f"E/V={m / n:.2f}"
        )
        edges = sorted(
            (min(idx[a], idx[b]), max(idx[a], idx[b])) for a, b in g.edges()
        )
        with open(os.path.join(OUT, f"{name}.edges"), "w") as f:
            f.write(f"# {name}: {n} vertices, {m} edges. 0-indexed, one 'u v' per line.\n")
            f.write("#\n")
            for line in CREDITS[name]:
                f.write(f"# {line}\n")
            f.write("# See NOTICE.md.\n")
            f.write(f"{n} {m}\n")
            for u, v in edges:
                f.write(f"{u} {v}\n")
    print("ALL MATCH" if ok else "SOME MISMATCH")


if __name__ == "__main__":
    main()
