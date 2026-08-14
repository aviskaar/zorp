# Build the zorp systems paper.
#
#   make          rebuild figures and the PDF
#   make paper    rebuild the PDF only
#   make figures  regenerate figures/ from current repo state
#   make clean    remove LaTeX intermediates (keeps the PDF)
#
# Needs pandoc, a TeX distribution with newtx (TeX Live works), and
# python3 with matplotlib.

PAPER := zorp-paper
TEMPLATE := arxiv-template.tex

.PHONY: all paper figures compat clean distclean

all: figures paper

figures:
	python3 make_figures.py

paper: $(PAPER).pdf

$(PAPER).pdf: $(PAPER).md $(TEMPLATE) references.bib
	pandoc $(PAPER).md -o $(PAPER).tex --template=$(TEMPLATE) --natbib
	pdflatex -interaction=nonstopmode -halt-on-error $(PAPER).tex >/dev/null
	bibtex $(PAPER) >/dev/null
	pdflatex -interaction=nonstopmode -halt-on-error $(PAPER).tex >/dev/null
	pdflatex -interaction=nonstopmode -halt-on-error $(PAPER).tex >/dev/null
	@$(MAKE) --no-print-directory compat
	@$(MAKE) --no-print-directory clean
	@echo "built $(PAPER).pdf"

# pdfTeX emits PDF 1.5 with object streams, which GitHub's inline PDF viewer
# renders inconsistently. Downconvert to a linearized PDF 1.4 (no object
# streams, all fonts embedded), the most broadly compatible form. No-op if
# Ghostscript is absent, so the plain pdflatex output is still produced.
compat:
	@if command -v gs >/dev/null 2>&1; then \
	  gs -sDEVICE=pdfwrite -dCompatibilityLevel=1.4 -dFastWebView=true \
	     -dEmbedAllFonts=true -dSubsetFonts=true -dAutoRotatePages=/None \
	     -dDetectDuplicateImages=true -dPDFSETTINGS=/prepress \
	     -o $(PAPER).compat.pdf $(PAPER).pdf >/dev/null 2>&1 \
	     && mv $(PAPER).compat.pdf $(PAPER).pdf \
	     && echo "linearized $(PAPER).pdf to PDF 1.4 for GitHub"; \
	else \
	  echo "gs not found; skipping PDF 1.4 downconvert (GitHub viewer may error)"; \
	fi

clean:
	@rm -f $(PAPER).aux $(PAPER).log $(PAPER).out $(PAPER).bbl \
	       $(PAPER).blg $(PAPER).tex

distclean: clean
	@rm -f $(PAPER).pdf
