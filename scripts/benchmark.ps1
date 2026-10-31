param([ValidateSet(10000,50000,100000)][int]$Count = 10000)
$ErrorActionPreference = 'Stop'
# 合成データはRustの一時ディレクトリ内に生成され、正常終了時に削除されます。
cargo run --release -p hub-core --example benchmark -- $Count
if ($LASTEXITCODE -ne 0) { throw 'Benchmark failed' }
