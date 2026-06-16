#!/bin/bash
# Download test fixtures for integration tests.
# Configuration: tests/fixtures/fixtures.toml
# Requires: gh (GitHub CLI), 7z (for PE extraction), unzip (for APK extraction)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR/.."
FIXTURES_DIR="$PROJECT_ROOT/tests/fixtures"
CONFIG_FILE="$FIXTURES_DIR/fixtures.toml"

# =============================================================================
# Configuration Parser
# =============================================================================

# Get field value for fixture at given index
get_field() {
    local index="$1"
    local field="$2"
    
    awk -v idx="$index" -v field="$field" '
        BEGIN { count = -1; in_block = 0 }
        /^\[\[fixture\]\]/ { count++; in_block = (count == idx) }
        in_block && $0 ~ "^" field " *= *\"" { 
            gsub(/^[a-z_]+ *= *"/, ""); 
            gsub(/"$/, ""); 
            print; 
            exit 
        }
        in_block && /^\[\[/ && !/^\[\[fixture\]\]/ { in_block = 0 }
    ' "$CONFIG_FILE"
}

# Count total fixtures
count_fixtures() {
    grep -c '^\[\[fixture\]\]' "$CONFIG_FILE" 2>/dev/null || echo 0
}

# =============================================================================
# Download Functions
# =============================================================================

download_macho() {
    local name="$1" repo="$2" version="$3" pattern="$4" extract_dir="$5" binary="$6"
    
    local target_dir="$FIXTURES_DIR/$extract_dir"
    local binary_path="$target_dir/$binary"
    
    if [ -f "$binary_path" ]; then
        echo "✅ $name (macho) - already exists"
        return 0
    fi
    
    echo "⬇️  Downloading $name (macho)..."
    mkdir -p "$target_dir"
    
    gh release download "$version" \
        --repo "$repo" \
        --pattern "$pattern" \
        --dir "$FIXTURES_DIR" --clobber
    
    tar -xzf "$FIXTURES_DIR/$pattern" -C "$target_dir"
    rm -f "$FIXTURES_DIR/$pattern"
    
    # Check exact path first
    if [ -f "$binary_path" ]; then
        echo "✅ $name (macho) - downloaded"
        return 0
    fi
    
    # Fallback: case-insensitive search (Linux is case-sensitive, macOS is not)
    local binary_name
    binary_name=$(basename "$binary")
    local found_binary
    found_binary=$(find "$target_dir" -iname "$binary_name" -type f | head -1 || true)
    
    if [ -n "$found_binary" ]; then
        # Create parent directories and symlink to match expected path
        mkdir -p "$(dirname "$binary_path")"
        ln -sf "$found_binary" "$binary_path"
        echo "✅ $name (macho) - downloaded (symlinked for case sensitivity)"
        return 0
    fi
    
    echo "❌ $name (macho) - binary not found: $binary"
    echo "   Expected path: $binary_path"
    echo "   Actual structure:"
    find "$target_dir" -type f 2>/dev/null | head -20 || true
    return 1
}

download_pe() {
    local name="$1" repo="$2" version="$3" pattern="$4" extract_dir="$5" binary="$6"
    
    if ! command -v 7z &> /dev/null; then
        echo "⚠️  7z not found. Skipping PE: $name"
        echo "   Install: brew install p7zip (macOS) / apt install p7zip-full (Linux)"
        return 0
    fi
    
    local target_dir="$FIXTURES_DIR/$extract_dir"
    local binary_path="$target_dir/$binary"
    
    if [ -f "$binary_path" ]; then
        echo "✅ $name (pe) - already exists"
        return 0
    fi
    
    echo "⬇️  Downloading $name (pe)..."
    mkdir -p "$target_dir"
    
    gh release download "$version" \
        --repo "$repo" \
        --pattern "$pattern" \
        --dir "$FIXTURES_DIR" --clobber
    
    # Extract from NSIS installer
    local temp_dir
    temp_dir=$(mktemp -d)
    7z x "$FIXTURES_DIR/$pattern" -o"$temp_dir" -y > /dev/null
    
    local found_exe
    found_exe=$(find "$temp_dir" -iname "$binary" -type f | head -1 || true)
    
    if [ -n "$found_exe" ]; then
        cp "$found_exe" "$binary_path"
        echo "✅ $name (pe) - downloaded"
    else
        echo "❌ $name (pe) - binary not found in installer: $binary"
        find "$temp_dir" -name "*.exe" -type f
        rm -rf "$temp_dir"
        rm -f "$FIXTURES_DIR/$pattern"
        return 1
    fi
    
    rm -rf "$temp_dir"
    rm -f "$FIXTURES_DIR/$pattern"
}

download_elf() {
    local name="$1" repo="$2" version="$3" pattern="$4" extract_dir="$5" binary="$6"

    if ! command -v unzip &> /dev/null; then
        echo "⚠️  unzip not found. Skipping ELF: $name"
        echo "   Install: brew install unzip (macOS) / apt install unzip (Linux)"
        return 0
    fi

    local target_dir="$FIXTURES_DIR/$extract_dir"
    local binary_path="$target_dir/$binary"

    if [ -f "$binary_path" ]; then
        echo "✅ $name (elf) - already exists"
        return 0
    fi

    echo "⬇️  Downloading $name (elf)..."
    mkdir -p "$(dirname "$binary_path")"

    if [ ! -f "$FIXTURES_DIR/$pattern" ]; then
        gh release download "$version" \
            --repo "$repo" \
            --pattern "$pattern" \
            --dir "$FIXTURES_DIR" --clobber
    fi

    if unzip -p "$FIXTURES_DIR/$pattern" "$binary" > "$binary_path"; then
        echo "✅ $name (elf) - downloaded"
    else
        echo "❌ $name (elf) - binary not found in APK: $binary"
        unzip -l "$FIXTURES_DIR/$pattern" | grep '\.so$' || true
        rm -f "$binary_path"
        rm -f "$FIXTURES_DIR/$pattern"
        return 1
    fi

    rm -f "$FIXTURES_DIR/$pattern"
}

# =============================================================================
# Main
# =============================================================================

download_fixture() {
    local index="$1"
    
    local name format repo version pattern extract_dir binary
    name=$(get_field "$index" "name")
    format=$(get_field "$index" "format")
    repo=$(get_field "$index" "repo")
    version=$(get_field "$index" "version")
    pattern=$(get_field "$index" "pattern")
    extract_dir=$(get_field "$index" "extract_dir")
    binary=$(get_field "$index" "binary")
    
    case "$format" in
        macho)
            download_macho "$name" "$repo" "$version" "$pattern" "$extract_dir" "$binary"
            ;;
        pe)
            download_pe "$name" "$repo" "$version" "$pattern" "$extract_dir" "$binary"
            ;;
        elf)
            download_elf "$name" "$repo" "$version" "$pattern" "$extract_dir" "$binary"
            ;;
        *)
            echo "⚠️  Unknown format: $format (fixture: $name)"
            ;;
    esac
}

main() {
    local filter="${1:-}"
    
    echo "📦 Test Fixtures Downloader"
    echo "   Config: $CONFIG_FILE"
    echo ""
    
    mkdir -p "$FIXTURES_DIR"
    
    local count
    count=$(count_fixtures)
    
    for ((i=0; i<count; i++)); do
        local format name
        format=$(get_field "$i" "format")
        name=$(get_field "$i" "name")
        
        # Filter by format if specified
        if [ -n "$filter" ] && [ "$filter" != "$format" ]; then
            continue
        fi
        
        download_fixture "$i"
    done
    
    echo ""
    echo "🎉 Done! Run: cargo test"
}

main "$@"
