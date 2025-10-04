check:
	cargo insta test

serve:
	trunk serve --release -a 0.0.0.0 --enable-cooldown
