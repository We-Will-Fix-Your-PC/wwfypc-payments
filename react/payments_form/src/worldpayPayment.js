import React, {Component} from 'react';
import 'whatwg-fetch';
import uuid from "uuid";
import * as Sentry from "@sentry/browser";
import SVG from "react-inlinesvg";
import loader from "./loader.svg";
import CardForm from "./cardForm";
import {API_ROOT} from "./payment";

const basicCardInstrument = {
    supportedMethods: 'basic-card',
    data: {
        supportedNetworks: [
            'visa', 'mastercard', 'amex'
        ]
    }
};

const appleMerchantID = 'merchant.uk.cardifftec';
const merchantName = 'We Will Fix Your PC';
const allowedAppleCardNetworks = ['visa', 'masterCard', 'amex'];

export default class WorldpayPayment extends Component {
    constructor(props) {
        super(props);

        this.state = {
            payment: null,
            err: null,
            selectedMethod: null,
            threedsData: null,
            accountData: null,
            popupWindow: null,
            loading: false,
            complete: false,
            canUsePaymentRequests: null,
            isApplePayReady: null,
            applePaySession: null,
        };

        this.handleError = this.handleError.bind(this);
        this.onComplete = this.onComplete.bind(this);
        this.updatePayment = this.updatePayment.bind(this);
        this.paymentTotal = this.paymentTotal.bind(this);
        this.paymentDetails = this.paymentDetails.bind(this);
        this.paymentOptions = this.paymentOptions.bind(this);
        this.applePaymentRequest = this.applePaymentRequest.bind(this);
        this.canUsePaymentRequests = this.canUsePaymentRequests.bind(this);
        this.makePaymentRequest = this.makePaymentRequest.bind(this);
        this.makeApplePayment = this.makeApplePayment.bind(this);
        this.validateApplePayment = this.validateApplePayment.bind(this);
        this.onFormSubmit = this.onFormSubmit.bind(this);
        this.takeBasicPayment = this.takeBasicPayment.bind(this);
        this.takeApplePayment = this.takeApplePayment.bind(this);
        this.takePayment = this.takePayment.bind(this);
        this.handlePaymentRequest = this.handlePaymentRequest.bind(this);
        this.handleMessage = this.handleMessage.bind(this);
        this.handleTryAgain = this.handleTryAgain.bind(this);
        this.openLoginPopup = this.openLoginPopup.bind(this);
    }

    updatePayment() {
        if (this.state.popupWindow) {
            this.state.popupWindow.close();
        }

        this.setState({
            payment: null,
            selectedMethod: null,
            threedsData: null,
            accountData: null,
            popupWindow: null,
            complete: false,
            loading: false,
            canUsePaymentRequests: null,
            isApplePayReady: null,
            applePaySession: null,
        });

        const checkMethods = (resp) => {
            this.canUsePaymentRequests()
                .then(value => this.setState({
                    canUsePaymentRequest: value
                }))
                .catch(err => this.handleError(err));
            if (resp.environment !== "TEST") {

            } else {
                if (window.ApplePaySession) {
                    if (window.ApplePaySession.canMakePayments()) {
                        this.setState({
                            isApplePayReady: true
                        });
                    } else {
                        this.setState({
                            isApplePayReady: false
                        });
                    }
                }
            }
        };

        if (this.props.paymentId) {
            fetch(`${API_ROOT}payment/${this.props.paymentId}/`, {
                credentials: 'include',
            })
                .then(resp => {
                    if (resp.ok) {
                        return resp.json();
                    } else {
                        throw new Error('Something went wrong');
                    }
                })
                .then(resp => {
                    if (resp.state !== "OPEN") {
                        this.handleError();
                        return;
                    }
                    this.setState({
                        payment: resp
                    });
                    checkMethods(resp);
                })
                .catch(err => this.handleError(err))
        } else {
            const payment = this.props.payment;
            payment.id = uuid.v4();
            payment.new = true;

            if (!payment.environment) {
                payment.environment = "TEST";
            }

            this.setState({
                payment: payment
            });

            checkMethods(payment);
        }
    }

    componentDidMount() {
        this.updatePayment();
        window.addEventListener("message", this.handleMessage, false);
    }

    handleError(err, message) {
        if (err) {
            console.error(err);
            const eventId = Sentry.captureException(err);
            this.setState({
                errId: eventId
            })
        }
        let error_msg = (message === undefined) ? "Something went wrong" : message;
        this.setState({
            err: error_msg,
            loading: false,
        })
    }

    paymentTotal() {
        return this.state.payment.items.reduce((prev, item) => prev + item.price, 0.0);
    }

    paymentDetails() {
        const total = this.paymentTotal();

        return {
            id: this.state.payment.id,
            total: {
                label: 'Total',
                amount: {
                    currency: 'GBP',
                    value: total
                }
            },
            displayItems: this.state.payment.items.map(item => {
                return {
                    label: item.title,
                    amount: {
                        currency: 'GBP',
                        value: item.price,
                    }
                }
            })
        }
    }

    paymentOptions() {
        return {
            requestPayerPhone: this.state.payment.customer.request_phone,
            requestPayerName:  this.state.payment.customer.request_name,
            requestPayerEmail:  this.state.payment.customer.request_email,
        }
    }

    canUsePaymentRequests() {
        return new Promise((resolve, reject) => {
            if (!window.PaymentRequest) {
                resolve(false)
            }
            let r = new window.PaymentRequest([basicCardInstrument], {
                total: {
                    label: 'Total',
                    amount: {
                        currency: 'GBP',
                        value: 0
                    }
                },
            }, {});
            r.canMakePayment()
                .then(c => resolve(c))
                .catch(e => reject(e));
        });
    }

    makePaymentRequest() {
        let methods = [basicCardInstrument];
        let request = new PaymentRequest(methods, this.paymentDetails(), this.paymentOptions());
        request.show()
            .then(res => {
                this.handlePaymentRequest(res)
            })
            .catch(err => {
                if (err.name === "NotSupportedError") {
                    this.setState({
                        selectedMethod: 'form'
                    });
                } else {
                    this.handleError(err)
                }
            })
    }

    handlePaymentRequest(res) {
        if (res.methodName === "basic-card") {
            this.takeBasicPayment(res);
        } else if (res.methodName === "https://google.com/pay") {
            this.takeGooglePayment(res);
        } else {
            res.complete('fail')
                .then(() => this.handleError(null))
                .catch(err => this.handleError(err));
        }
    }

    applePaymentRequest() {
        const paymentDataRequest = {
            countryCode: 'GB',
            currencyCode: 'GBP',
            supportedNetworks: allowedAppleCardNetworks,
            merchantCapabilities: ['supports3DS', 'supportsEMV', 'supportsCredit', 'supportsDebit'],
            requiredBillingContactFields: ["postalAddress", "email", "phone", "name"],
            total: {label: merchantName, amount: this.paymentTotal().toString()},
            lineItems: this.state.payment.items.map(item => {
                return {
                    type: "final",
                    label: item.title,
                    amount: item.price,
                }
            })
        };
        if (this.state.payment.customer !== null) {
            paymentDataRequest.billingContact = {
                emailAddress: this.state.payment.customer.email,
                phoneNumber: this.state.payment.customer.phone,
                givenName: this.state.payment.customer.name,
            };
        }
        return paymentDataRequest;
    }

    makeApplePayment() {
        const paymentDataRequest = this.applePaymentRequest();
        const session = new window.ApplePaySession(1, paymentDataRequest);
        session.onvalidatemerchant = this.validateApplePayment;
        session.onpaymentauthorized = this.takeApplePayment;
        session.oncancel = () => {
            this.setState({
                applePaySession: null
            })
        };

        this.setState({
            applePaySession: session
        });

        session.begin();
        console.log(session);
    }

    validateApplePayment(event) {
        fetch(`${API_ROOT}apple-merchant-verification/`, {
            method: "POST",
            credentials: 'include',
            body: JSON.stringify({
                url: event.validationURL
            }),
            headers: {
                "Content-Type": "application/json"
            }
        })
            .then(resp => {
                if (resp.ok) {
                    return resp.json();
                } else {
                    throw new Error('Something went wrong');
                }
            })
            .then(resp => {
                console.log(resp);
                this.state.applePaySession.completeMerchantValidation(resp.verification);
            })
            .catch(err => {
                this.state.applePaySession.abort();
                this.handleError(err)
            });
    }

    onFormSubmit(res) {
        this.setState({
            loading: true,
        });
        this.handlePaymentRequest(res);
    }

    takeBasicPayment(res) {
        let data = {
            card: {
                name: res.details.cardholderName,
                exp_month: res.details.expiryMonth,
                exp_year: res.details.expiryYear,
                card_number: res.details.cardNumber,
                cvc: res.details.cardSecurityCode,
            },
            billing_address: res.details.billingAddress,
            email: res.payerEmail,
            phone: res.payerPhone,
            name: res.payerName
        };
        this.takePayment(res, data);
    }

    takeApplePayment(event) {
        console.log(event);
        let res = {
            complete: (status) => new Promise((resolve, reject) => {
                if (status === "success") {
                    this.state.applePaySession.completePayment({
                        status: 0
                    })
                } else {
                    this.state.applePaySession.completePayment({
                        status: 1
                    })
                }
                resolve(true);
            })
        };
        let data = {
            billingAddress: {
                addressLine: event.billingContact.addresLines,
                country: event.billingContact.countryCode,
                city: event.billingContact.locality,
                dependentLocality: "",
                organization: "",
                phone: event.billingContact.phoneNumber,
                postalCode: event.billingContact.postalCode,
                recipient: event.billingContact.givenName + " " + event.billingContact.familyName,
                region: event.billingContact.administrativeArea,
                regionCode: "",
                sortingCode: billingAddress.sortingCode
            },
            first_name: event.billingContact.givenName,
            last_name: event.billingContact.familyName,
            appleData: event.token,
        };
        console.log(data);
        this.takePayment(res, data);
    }

    takePayment(res, data) {
        data.accepts = this.props.acceptsHeader;
        if (data.name) {
            const name = data.name.split(" ");
            data.first_name = name.splice(-1)[0];
            data.last_name = name.join(" ");
        }

        if (this.state.payment.new) {
            data.payment = this.state.payment;
        }

        fetch(`${API_ROOT}payment/worldpay/${this.state.payment.id}/`, {
            method: "POST",
            credentials: 'include',
            body: JSON.stringify(data),
            headers: {
                "Content-Type": "application/json"
            }
        })
            .then(resp => {
                if (resp.ok) {
                    return resp.json();
                } else {
                    throw new Error('Something went wrong');
                }
            })
            .then(resp => {
                if (resp.state === "SUCCESS") {
                    res.complete('success')
                        .then(() => {
                            this.onComplete(data.email ? data.email : this.state.payment.customer.email);
                        })
                        .catch(err => this.handleError(err))
                } else if (resp.state === "3DS") {
                    res.complete('success')
                        .then(() => {
                            this.setState({
                                threedsData: resp,
                                email: data.email ? data.email : this.state.payment.customer.email
                            });
                        })
                        .catch(err => this.handleError(err))
                } else if (resp.state === "EXISTING_ACCOUNT") {
                    res.complete('success')
                        .then(() => {
                            this.setState({
                                accountData: {
                                    resp: resp,
                                    data: data,
                                }
                            });
                        })
                        .catch(err => this.handleError(err))
                } else if (resp.state === "FAILED") {
                    res.complete('fail')
                        .then(() => {
                            this.handleError(null, "Payment failed")
                        })
                        .catch(err => this.handleError(err))
                } else {
                    this.handleError(null)
                }
            })
            .catch(err => {
                res.complete('fail')
                    .then(() => {
                        this.handleError(null, "Payment failed")
                    })
                    .catch(err => this.handleError(err))
            })
    }

    onComplete(email) {
        this.setState({
            complete: true,
            loading: false,
        });
        this.props.onComplete(this.state.payment.id, email)
    }

    handleMessage(event) {
        if (event.data.type === "3DS") {
            if (this.state.threedsData !== null) {
                if (event.data.payment_id !== this.state.payment.id) {
                    this.handleError();
                }
                if (event.data.threeds_approved) {
                    this.onComplete(this.state.email);
                } else {
                    this.handleError(null, "Payment failed");
                }
            }
        } else if (event.data.type === "login") {
            if (this.state.accountData !== null) {
                if (!event.data.login_successful) {
                    this.handleError(null, "Login failed");
                } else {
                    if (this.state.popupWindow) {
                        this.state.popupWindow.close();
                    }
                    const data = this.state.accountData.data;

                    this.setState({
                        loading: true,
                        accountData: null,
                    });

                    this.takePayment({
                        payerPhone: data.phone,
                        payerEmail: data.email,
                        payerName: data.payerName,
                        complete: () => Promise.resolve({})
                    }, data)
                }
            }
        }
    }

    handleTryAgain(e) {
        e.preventDefault();
        this.setState({
            err: null
        });
        this.updatePayment();
    }

    openLoginPopup() {
        const win = window.open(
            this.state.accountData.resp.frame,
            'login',
            'status=no,location=no,toolbar=no,menubar=no'
        );

        this.setState({
            popupWindow: win
        });
    }

    render() {
        if (this.state.err != null) {
            return <React.Fragment>
                <h3>{this.state.err}</h3>
                <div className="buttons">
                    <button onClick={this.handleTryAgain}>Try again</button>
                    {this.state.errId ? <button onClick={() => {
                        Sentry.showReportDialog({eventId: this.state.errId})
                    }}>Report feedback</button> : null}
                </div>
            </React.Fragment>;
        } else if (this.state.complete) {
            return <h3>Payment successful</h3>;
        } else if (this.state.payment === null || this.state.canUsePaymentRequest === null) {
            return <SVG src={loader} className="loader"/>
        } else if (this.state.threedsData !== null) {
            return <iframe src={this.state.threedsData.frame} width={390} height={400}/>
        } else if (this.state.accountData !== null) {
            return <React.Fragment>
                <h3>An account already exists with that email, please login to continue</h3>
                <div className="buttons">
                    <button onClick={this.openLoginPopup}>Login</button>
                </div>
            </React.Fragment>;
        } else {
            if (this.state.selectedMethod !== "form" && (this.state.isApplePayReady || this.state.canUsePaymentRequest)) {
                return <div className="buttons">
                    {this.state.isApplePayReady ?
                        <div className="apple-pay-button-with-text apple-pay-button-black-with-text"
                             onClick={this.makeApplePayment}>
                            <span className="text">Buy with</span>
                            <span className="logo"/>
                        </div> : null}
                    {this.state.canUsePaymentRequest ? <button onClick={this.makePaymentRequest}>
                        Autofill from browser
                    </button> : null}
                    <a href="" onClick={(e) => {
                        e.preventDefault();
                        this.setState({selectedMethod: "form"})
                    }}>
                        Enter card details manually
                    </a>
                </div>;
            } else {
                if (!this.state.loading) {
                    const paymentOptions = this.paymentOptions();

                    return <CardForm paymentOptions={paymentOptions} payment={this.state.payment}
                                     onSubmit={this.onFormSubmit}/>;
                } else {
                    return <SVG src={loader} className="loader"/>
                }
            }
        }
    }
}
