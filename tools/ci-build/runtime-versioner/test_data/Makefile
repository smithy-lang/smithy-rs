all:
	echo "Use the pack/unpack targets."

# Unpacks test_base.git.tar.gz
#
# Note: doesn't directly clone the bare repository into a working tree,
# or otherwise it would lose all the different branches.
unpack:
	rm -rf test_base
	mkdir -p test_base
	tar xfz test_base.git.tar.gz
	mv test_base.git test_base/.git
	(cd test_base && git config --unset core.bare)
	(cd test_base && git reset --hard)

# Packs test_base.git.tar.gz
pack:
	git clone --bare test_base
	tar cfz test_base.git.tar.gz test_base.git
	rm -rf test_base.git

.PHONY: all