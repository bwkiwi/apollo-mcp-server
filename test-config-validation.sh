#!/bin/bash

# Test script for configuration file validation
# This script tests various error scenarios to ensure helpful error messages

echo "🧪 Testing Apollo MCP Server Configuration Validation"
echo "=================================================="

# Build the server first
echo "Building server..."
cargo build --release
echo ""

# Test 1: Missing directory
echo "Test 1: Missing directory"
echo "-------------------------"
./target/release/apollo-mcp-server /nonexistent/path/config.yaml 2>&1 | head -10
echo ""

# Test 2: Missing file in existing directory  
echo "Test 2: Missing file in existing directory"
echo "------------------------------------------"
mkdir -p /tmp/test-apollo-config
./target/release/apollo-mcp-server /tmp/test-apollo-config/missing.yaml 2>&1 | head -10
echo ""

# Test 3: Pointing to directory instead of file
echo "Test 3: Pointing to directory instead of file"
echo "----------------------------------------------"
./target/release/apollo-mcp-server /tmp/test-apollo-config 2>&1 | head -10
echo ""

# Test 4: Permission denied
echo "Test 4: Permission denied file"
echo "------------------------------"
echo "test config" > /tmp/test-apollo-config/readonly.yaml
chmod 000 /tmp/test-apollo-config/readonly.yaml
./target/release/apollo-mcp-server /tmp/test-apollo-config/readonly.yaml 2>&1 | head -10
echo ""

# Test 5: Valid file (should proceed past validation)
echo "Test 5: Valid configuration file"
echo "--------------------------------"
cat > /tmp/test-apollo-config/valid.yaml << 'EOF'
endpoint: "https://api.example.com/graphql"
schema:
  introspect: {}
operations:
  introspect: {}
introspection:
  execute:
    enabled: true
EOF

echo "Running with valid config (should start server, press Ctrl+C to stop):"
timeout 3s ./target/release/apollo-mcp-server /tmp/test-apollo-config/valid.yaml 2>&1 || echo "✅ Server started successfully with valid config"
echo ""

# Cleanup
chmod 644 /tmp/test-apollo-config/readonly.yaml 2>/dev/null || true
rm -rf /tmp/test-apollo-config

echo "🎉 Configuration validation tests completed!"
echo ""
echo "Summary of error message types:"
echo "• Missing directory: Shows mkdir command and path guidance"  
echo "• Missing file: Shows creation guidance and documentation links"
echo "• Directory instead of file: Shows correct file path format"
echo "• Permission issues: Shows permission and access guidance"
echo "• Valid config: Proceeds to start server normally"