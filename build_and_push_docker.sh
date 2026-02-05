#!/bin/bash
set -e

echo "Building Docker image..."
docker build -t ghcr.io/starkbotai/starkbot:flash -t ghcr.io/starkbotai/starkbot:latest .

echo "Pushing to registry..."
docker push ghcr.io/starkbotai/starkbot:flash
docker push ghcr.io/starkbotai/starkbot:latest

echo "Done!"
