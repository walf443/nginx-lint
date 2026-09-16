NGINX_LINT := ../../target/debug/nginx-lint
FIX_KINDS := tests/fix-kinds

.PHONY: test test-e2e clean

# Unit tests, including the committed runtime's freshness checks
test:
	cargo test -p nginx-lint-plugin-sdk

# What the unit tests cannot reach: a plugin built by this tool, run by the
# host. tests/fix-kinds uses every fix builder in nginx_lint.lua once;
# test-plugins requires its fixes to resolve the finding, and the diff
# requires them to produce exactly good.conf — the Lua fix arithmetic is a
# reimplementation of the host's, and this is what pins the two together.
test-e2e:
	cargo run -q -p nginx-lint-plugin-sdk -- build $(FIX_KINDS)/plugin.lua -o $(FIX_KINDS)/fix-kinds.wasm
	$(NGINX_LINT) test-plugins --plugins $(FIX_KINDS)
	@tmp=$$(mktemp -d); cp $(FIX_KINDS)/bad.conf $$tmp/nginx.conf; \
		$(NGINX_LINT) --plugins $(FIX_KINDS) --rule-only fix-kinds --fix $$tmp/nginx.conf >/dev/null; \
		diff $(FIX_KINDS)/good.conf $$tmp/nginx.conf && echo "  --fix produced good.conf"; \
		status=$$?; rm -rf $$tmp; exit $$status

clean:
	rm -f $(FIX_KINDS)/fix-kinds.wasm
