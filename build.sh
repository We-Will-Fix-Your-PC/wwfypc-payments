#!/usr/bin/env bash

PAYMENT_PROVIDER="WORLDPAY";
export PAYMENT_PROVIDER

VERSION=$(sentry-cli releases propose-version || exit)

cd react/payments_form || exit
yarn webpack --config webpack.prod.js || exit
cd ../admin_portal
yarn webpack --config webpack.prod.js || exit
cd ../..

docker build -t "theenbyperor/wwfypc-payments:$VERSION" . || exit
docker push "theenbyperor/wwfypc-payments:$VERSION" || exit

sentry-cli releases --org we-will-fix-your-pc new -p payments-server $VERSION || exit
sentry-cli releases --org we-will-fix-your-pc set-commits --auto $VERSION