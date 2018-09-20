PID=`getpid pizza_freak`
cargo build --release
if [ $? -ne 0 ]; then
    exit $?
fi
if [ -z "$PID"]; then
    kill -kill $PID
fi
RUST_LOG=pizza_freak:info,pizza_freak:error ./target/release/pizza_freak > ~/logs/pizza_freak.log &