class Cxms < Formula
  desc "High-performance CLI for searching Codex session rollout JSONL files"
  homepage "https://github.com/mkusaka/cxms"
  version "__VERSION__"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "__DARWIN_ARM64_URL__"
      sha256 "__DARWIN_ARM64_SHA256__"
    else
      url "__DARWIN_X64_URL__"
      sha256 "__DARWIN_X64_SHA256__"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "__LINUX_ARM64_URL__"
      sha256 "__LINUX_ARM64_SHA256__"
    else
      url "__LINUX_X64_URL__"
      sha256 "__LINUX_X64_SHA256__"
    end
  end

  def install
    bin.install "cxms"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cxms --version")
  end
end
