# Requires @mermaid-js/mermaid-cli to be installed
# Builds diagrams from the mermaid sources
DIAGRAMS=$(wildcard *.mmd)
SVGS=$(DIAGRAMS:.mmd=.svg)
OPTIONS=-t dark -b transparent

all: $(SVGS)

%.svg: %.mmd
	mmdc -i $*.mmd -o $*.svg $(OPTIONS)

clean:
	rm -f $(SVGS)
