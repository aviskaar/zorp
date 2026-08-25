# Build the zorp executive summary (a two-page PDF).
#
#   make          build exec-summary.pdf
#   make clean    remove LaTeX intermediates (keeps the PDF)
#
# Needs pandoc and a TeX distribution (pdflatex). The figure is reused
# from docs/paper/figures so there is exactly one copy of it.

PANDOC := pandoc
PDF := zorp-exec-summary

.PHONY: all clean

all: $(PDF).pdf

$(PDF).pdf: exec-summary.md header.tex ../paper/figures/pipeline.png
	$(PANDOC) exec-summary.md -o $@ \
	  --pdf-engine=pdflatex \
	  -H header.tex \
	  -V documentclass=article -V fontsize=10pt \
	  -V geometry:letterpaper,margin=0.62in \
	  -V colorlinks=true -V linkcolor=zorpblue -V urlcolor=zorpblue \
	  --resource-path=.:../paper

clean:
	@rm -f $(PDF).aux $(PDF).log $(PDF).out
