-include .env

build:
	clear
	cargo build --release
	sudo cp $(shell pwd)/target/release/chyp /usr/local/bin/chyp
	sudo chown $(USER):$(USER) /usr/local/bin/chyp

