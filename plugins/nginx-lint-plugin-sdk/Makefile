NGINX_LINT := ../../target/debug/nginx-lint
FIX_KINDS := tests/fix-kinds
TWO_RULES := tests/two-rules
FAILING := tests/failing-rules

.PHONY: test test-e2e clean

# Unit tests, including the committed runtime's freshness checks
test:
	cargo test -p nginx-lint-plugin-sdk

# What the unit tests cannot reach: a plugin built by this tool, run by the
# host. tests/fix-kinds uses every fix builder in nginx_lint.lua once;
# test-plugins requires its fixes to resolve the finding, and the diff
# requires them to produce exactly good.conf — the Lua fix arithmetic is a
# reimplementation of the host's, and this is what pins the two together.
#
# tests/two-rules returns a list of two rules: test-plugins has to load it as
# two, check each against its own examples, and the fixes of both, asked
# for together, have to produce good.conf. tests/failing-rules has a rule
# that returns a malformed finding and one that throws beside a working
# one: each failure is that rule's own finding, and the working rule's
# finding survives.
test-e2e:
	cargo run -q -p nginx-lint-plugin-sdk -- build $(FIX_KINDS)/plugin.lua -o $(FIX_KINDS)/fix-kinds.wasm
	$(NGINX_LINT) test-plugins --plugins $(FIX_KINDS)
	@tmp=$$(mktemp -d); cp $(FIX_KINDS)/bad.conf $$tmp/nginx.conf; \
		$(NGINX_LINT) --plugins $(FIX_KINDS) --rule-only fix-kinds --fix $$tmp/nginx.conf >/dev/null; \
		diff $(FIX_KINDS)/good.conf $$tmp/nginx.conf && echo "  --fix produced good.conf"; \
		status=$$?; rm -rf $$tmp; exit $$status
	cargo run -q -p nginx-lint-plugin-sdk -- build $(TWO_RULES)/plugin.lua -o $(TWO_RULES)/two-rules.wasm
	$(NGINX_LINT) test-plugins --plugins $(TWO_RULES) 2>&1 | tee /dev/stderr | grep -q '^2 rule(s), 6 check(s) passed'
	@tmp=$$(mktemp -d); cp $(TWO_RULES)/bad.conf $$tmp/nginx.conf; \
		$(NGINX_LINT) --plugins $(TWO_RULES) --rule-only server-tokens-enabled-two,autoindex-enabled-two --fix $$tmp/nginx.conf >/dev/null; \
		diff $(TWO_RULES)/good.conf $$tmp/nginx.conf && echo "  --fix produced good.conf with both rules"; \
		status=$$?; rm -rf $$tmp; exit $$status
	cargo run -q -p nginx-lint-plugin-sdk -- build $(FAILING)/plugin.lua -o $(FAILING)/failing-rules.wasm
	@out=$$($(NGINX_LINT) --plugins $(FAILING) --rule-only ok-rule,shape-rule,throw-rule $(FAILING)/nginx.conf 2>&1); \
		echo "$$out" | grep -q 'warning\[c/ok-rule\]: found' && \
		echo "$$out" | grep -q 'error\[c/shape-rule\]: finding 2 returned by check() is a number' && \
		echo "$$out" | grep -q 'error\[c/throw-rule\]: plugin.lua:[0-9]*: boom' && \
		echo "  each failing rule is reported as its own finding" || { echo "$$out"; exit 1; }

clean:
	rm -f $(FIX_KINDS)/fix-kinds.wasm $(TWO_RULES)/two-rules.wasm $(FAILING)/failing-rules.wasm
