# test example

cargo run -p fynkel_cli -- run --project examples/showcase

cd examples/showcase && ../../target/debug/fynkel run

cargo install --path apps/cli
cd examples/showcase && fynkel run
