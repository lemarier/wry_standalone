set -e

if ! command -v unzip >/dev/null; then
	echo "Error: unzip is required to install wry." 1>&2
	exit 1
fi

if [ "$OS" = "Windows_NT" ]; then
	target="x86_64-pc-windows-msvc"
else
	case $(uname -sm) in
	"Darwin x86_64") target="x86_64-apple-darwin" ;;
	"Darwin arm64") target="aarch64-apple-darwin" ;;
	*) target="x86_64-unknown-linux-gnu" ;;
	esac
fi

if [ $# -eq 0 ]; then
	wry_uri="https://github.com/lemarier/wry_standalone/releases/latest/download/wry-${target}.zip"
else
	wry_uri="https://github.com/lemarier/wry_standalone/releases/download/${1}/wry-${target}.zip"
fi

wry_install="${wry_INSTALL:-$HOME/.wry}"
bin_dir="$wry_install/bin"
exe="$bin_dir/wry"

if [ ! -d "$bin_dir" ]; then
	mkdir -p "$bin_dir"
fi

curl --fail --location --progress-bar --output "$exe.zip" "$wry_uri"
unzip -d "$bin_dir" -o "$exe.zip"
chmod +x "$exe"
rm "$exe.zip"

echo "wry was installed successfully to $exe"
if command -v wry >/dev/null; then
	echo "Run 'wry --help' to get started"
else
	case $SHELL in
	/bin/zsh) shell_profile=".zshrc" ;;
	*) shell_profile=".bash_profile" ;;
	esac
	echo "Manually add the directory to your \$HOME/$shell_profile (or similar)"
	echo "  export WRY_INSTALL=\"$wry_install\""
	echo "  export PATH=\"\$WRY_INSTALL/bin:\$PATH\""
	echo "Run '$exe --help' to get started"
fi