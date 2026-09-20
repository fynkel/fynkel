# test example

cargo run -p fynkel_cli -- run --project examples/showcase

cd examples/showcase && ../../target/debug/fynkel run

cargo install --path apps/cli
cd examples/showcase && fynkel run

# run stress test (the project directory is generated and git-ignored:
# --app wraps it in the starter app with the orbiting camera; run once,
# and again after editing the generator)

python3 benchmarks/stress/generate.py --app
cargo run -p fynkel_cli -- run --project benchmarks/stress/project            # this Mac
cargo run -p fynkel_cli -- run --project benchmarks/stress/project -d <iphone>
