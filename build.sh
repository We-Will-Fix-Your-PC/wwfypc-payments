#!/usr/bin/env bash

PAYMENT_PROVIDER="WORLDPAY";
export PAYMENT_PROVIDER

#VERSION=$(sentry-cli releases propose-version || exit)

cd react/payments_form || exit
yarn webpack --config webpack.prod.js || exit
cd ../..

cargo build --release

#sentry-cli releases --org we-will-fix-your-pc new -p bot-server $VERSION || exit
#sentry-cli releases --org we-will-fix-your-pc set-commits --auto $VERSION