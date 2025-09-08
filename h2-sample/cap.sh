#!/bin/bash

# Default values
PORT="8000"
OUTPUT_FILE="capture"

# Parse named arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --port)
      PORT="$2"
      shift 2
      ;;
    --file)
      OUTPUT_FILE="$2"
      shift 2
      ;;
    --help|-h)
      echo "Usage: $0 [--port PORT] [--file FILENAME]"
      echo "Captures traffic on lo0"
      echo "  --port PORT      Port to capture (default: 8000)"
      echo "  --file FILENAME  Name of the pcap file (default: capture)"
      exit 0
      ;;
    *)
      echo "Unknown option $1"
      echo "Use --help for usage information"
      exit 1
      ;;
  esac
done

echo "Capturing traffic on lo0 port $PORT to file: $OUTPUT_FILE.pcap"
echo "Press Ctrl+C to stop capturing"

# Run tcpdump with the determined output file and port
tcpdump -i lo0 -w "$OUTPUT_FILE.pcap" port $PORT
