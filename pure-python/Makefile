CURRENT_DIR := $(shell pwd)

.PHONY: run-mcp
run-mcp:
	mcp run server.py -t sse

.PHONY: run-explorer
run-explorer:
	docker stop kuzu-explorer && docker rm kuzu-explorer
	docker run --name kuzu-explorer \
           -p 8888:8000 \
           -v $(CURRENT_DIR)/graph/db:/database \
           -d kuzudb/explorer:0.10.0
