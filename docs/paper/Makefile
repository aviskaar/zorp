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

.PHONY: all paper figures clean distclean

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
	@$(MAKE) --no-print-directory clean
	@echo "built $(PAPER).pdf"

clean:
	@rm -f $(PAPER).aux $(PAPER).log $(PAPER).out $(PAPER).bbl \
	       $(PAPER).blg $(PAPER).tex

distclean: clean
	@rm -f $(PAPER).pdf
