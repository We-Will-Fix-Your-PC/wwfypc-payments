import React, {Component} from 'react';
import {faCcVisa, faCcMastercard, faCcAmex} from '@fortawesome/free-brands-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { AsYouType } from "libphonenumber-js";
import Payment from 'payment';

const brands = {
    "amex": {
        colour: "#108168",
        icon: faCcAmex
    },
    "visa": {
        colour: "#191278",
        icon: faCcVisa
    },
    "mastercard": {
        colour: "#ff5f00",
        icon: faCcMastercard
    },
    "monzo": {
        colour: "#ff4d56",
        icon: faCcMastercard
    }
};

export default class CardForm extends Component {
    constructor(props) {
        super(props);

        this.state = {
            card_type: null,
            card_number: "",
            card_display_number: "•••• •••• •••• ••••",
            card_name: "",
            card_display_name: "NAME",
            card_expiry: "",
            card_display_expiry: "Expiry: •• / ••",
            card_cvc: "",
            card_display_cvc: "•••",
            exp_month: null,
            exp_year: null,
            name_focused: false,
            number_focused: false,
            expiry_focused: false,
            cvc_focused: false,
            tel: '',
            telValid: false,
            telNum: null,
            email: '',
            emailValid: false,
        };

        this.updateCardName = this.updateCardName.bind(this);
        this.updateCardNumber = this.updateCardNumber.bind(this);
        this.updateCardExpiry = this.updateCardExpiry.bind(this);
        this.updateCardCvc = this.updateCardCvc.bind(this);
        this.handleTelChange = this.handleTelChange.bind(this);
        this.handleEmailChange = this.handleEmailChange.bind(this);
        this.handleSubmit = this.handleSubmit.bind(this);
    }

    setCardType(event) {
        const type = Payment.fns.cardType(event.target.value);
        this.setState({
            card_type: type
        })
    }

    handleSubmit(event) {
        event.preventDefault();

        const name = this.state.card_name;
        const number = this.state.card_number;
        const exp_month = this.state.exp_month;
        const exp_year = this.state.exp_year;
        const cvc = this.state.card_cvc;
        const paymentResponse = {
            details: {
                billingAddress: {
                    addressLine: [],
                    country: "",
                    city: "",
                    dependentLocality: "",
                    organization: "",
                    phone: this.state.telNum ? this.state.telNum.format("E.164"): this.props.payment.customer.phone,
                    postalCode: "",
                    recipient: name,
                    region: "",
                    regionCode: "",
                    sortingCode: ""
                },
                cardNumber: number,
                cardholderName: name,
                cardSecurityCode: cvc,
                expiryMonth: exp_month,
                expiryYear: exp_year
            },
            methodName: "basic-card",
            payerName: name,
            complete: () => Promise.resolve({})
        };
        if (this.props.paymentOptions.requestPayerPhone) {
            paymentResponse.payerPhone = this.state.telNum.format("E.164");
        }
        if (this.props.paymentOptions.requestPayerEmail) {
            paymentResponse.payerEmail = this.state.email;
        }
        this.props.onSubmit(paymentResponse);
    }

    updateCardName(e) {
        let new_val = e.target.value;
        let new_display_val = new_val;
        if (new_display_val.length === 0) {
            new_display_val = "NAME"
        }
        if (!new_val.length) {
            this.refs.number.setCustomValidity("Enter a name");
        } else {
            this.refs.number.setCustomValidity("");
        }
        this.setState({
            card_name: new_val,
            card_display_name: new_display_val
        })
    }

    updateCardNumber(e) {
        let new_val = e.target.value;
        new_val = new_val.replace(/[^0-9]+/g, "");
        if (new_val.length > 16) {
            new_val = new_val.slice(0, 16)
        }
        let new_display_val = new_val;
        while (new_display_val.length !== 16) {
            new_display_val += "•";
        }
        let r = (cur, next, i) => {
            cur += next;
            if (i % 4 === 3) {
                cur += " ";
            }
            return cur;
        };
        new_display_val = new_display_val.split("").reduce(r, "");
        let type = Payment.fns.cardType(new_val);
        if (type && !brands[type]) {
            this.refs.number.setCustomValidity("Unsupported card type");
            return
        } else if (!Payment.fns.validateCardNumber(new_val)) {
            this.refs.number.setCustomValidity("Invalid card number");
        } else {
            this.refs.number.setCustomValidity("");
        }
        if (new_val.startsWith("535522")) {
            type = "monzo";
        }
        this.setState({
            card_type: type,
            card_number: new_val,
            card_display_number: new_display_val
        })
    }

    updateCardExpiry(e) {
        let new_val = e.target.value;
        new_val = new_val.replace(/[^0-9/]+/g, "");
        let parts = new_val.split("/");
        if (parts.length > 2) {
            parts = parts.slice(0, 2)
        }
        let had_slash = parts.length >= 2;
        let month = null;
        let month_int = null;
        let year = null;
        let year_int = null;
        if (parts[0].trim().length > 0) {
            month = parts[0].trim();
            if (month.length > 2) {
                month = month.slice(0, 2);
            }
            month_int = parseInt(month);
            if (month_int > 12) {
                month_int = 12;
            }
            if (parts.length > 1 && parts[1].trim().length > 0) {
                year = parts[1].trim();
                if (year.length > 4) {
                    year = year.slice(0, 4);
                }
                if (year.length === 2) {
                    year_int = (Math.floor(new Date().getFullYear() / 1000) * 1000) + parseInt(year);
                } else {
                    year_int = parseInt(year)
                }
            }
        }
        new_val = "";
        let new_display_val = "Expiry: ";
        if (month !== null && month_int !== null) {
            new_val += month;
            new_display_val += month_int.toString().padStart(2, "0") + " / ";
            if (had_slash) {
                new_val += "/";
            }
            if (year !== null) {
                if (!had_slash) {
                    new_val += "/";
                }
                new_val += year;
                new_display_val += (year_int % 1000).toString().padStart(2, "0");
                let a = new Date();
                if (year_int < a.getFullYear() || (year_int === a.getFullYear() && month_int <= a.getMonth())) {
                    this.refs.expiry.setCustomValidity("Date is in the past");
                } else {
                    this.refs.expiry.setCustomValidity("");
                }
            } else {
                new_display_val += "••";
                this.refs.expiry.setCustomValidity("Enter a year");
            }
        } else {
            new_display_val += "•• / ••";
            this.refs.expiry.setCustomValidity("Enter expiry");
        }
        this.setState({
            card_expiry: new_val,
            card_display_expiry: new_display_val,
            exp_month: month_int,
            exp_year: year_int
        })
    }

    updateCardCvc(e) {
        let new_val = e.target.value;
        new_val = new_val.replace(/[^0-9]+/g, "");
        let wanted_len = 3;
        if (this.state.card_type === "amex") {
            wanted_len = 4;
        }
        if (new_val.length > wanted_len) {
            new_val = new_val.slice(0, wanted_len)
        }
        let new_display_val = new_val;
        while (new_display_val.length !== wanted_len) {
            new_display_val += "•";
        }
        if (new_val.length !== wanted_len) {
            this.refs.cvc.setCustomValidity("Invalid CVC");
        } else {
            this.refs.cvc.setCustomValidity("");
        }
        this.setState({
            card_cvc: new_val,
            card_display_cvc: new_display_val
        })
    }


    static valid_email(email) {
        return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)
    }

    handleTelChange(event) {
        let new_val = event.target.value;
        let formatter = new AsYouType('GB');
        let val = formatter.input(new_val);
        let num = formatter.getNumber();
        let valid = formatter.isValid();
        if (!valid) {
            this.refs.tel.setCustomValidity("Invalid phone number");
        } else {
            this.refs.tel.setCustomValidity("");
        }
        this.setState({
            tel: val,
            telNum: num,
            telValid: valid,
        });
    }

    handleEmailChange(event) {
        let new_val = event.target.value;
        let valid = false;
        if (!CardForm.valid_email(new_val)) {
            this.refs.email.setCustomValidity("Invalid email");
        } else {
            this.refs.email.setCustomValidity("");
            valid = true;
        }
        this.setState({
            email: new_val,
            emailValid: valid
        });
    }

    render() {
        return <div className="CardForm">
            <form onSubmit={this.handleSubmit}>
                {(this.props.paymentOptions.requestPayerPhone) ?
                    <input className="phone" type="tel" ref="tel" value={this.state.tel} onChange={this.handleTelChange} placeholder="Phone number" required/> : null}
                {(this.props.paymentOptions.requestPayerEmail) ?
                    <input className="email" type="email" ref="email" value={this.state.email} onChange={this.handleEmailChange} placeholder="Email address" required/> : null}
                <div className="disp-card">
                    <div className={"disp-card-inner " + this.state.card_type + (this.state.cvc_focused ? " flip" : "")}>
                        <div className="disp-card-front" style={this.state.card_type ? {
                            backgroundColor: brands[this.state.card_type].colour
                        } : {}}>
                            {this.state.card_type ? <FontAwesomeIcon icon={brands[this.state.card_type].icon} size="3x" className="brand-icon"/> : null }
                            <div className="disp-card-pads"/>
                            <div className={"disp-card-number" + (this.state.number_focused ? " focus": "")}>
                                {this.state.card_display_number}
                            </div>
                            <div className={"disp-card-name" + (this.state.name_focused ? " focus": "")}>
                                {this.state.card_display_name}
                            </div>
                            <div className={"disp-card-expiry" + (this.state.expiry_focused ? " focus": "")}>
                                {this.state.card_display_expiry}
                            </div>
                            <div className={"disp-card-cvc" + (this.state.cvc_focused ? " focus": "")}>
                                {this.state.card_display_cvc}
                            </div>
                        </div>
                        <div className="disp-card-back" style={this.state.card_type ? {
                            backgroundColor: brands[this.state.card_type].colour
                        } : {}}>
                            <div className="disp-card-magstripe" />
                            <div className="disp-card-signature" />
                            <div className={"disp-card-cvc" + (this.state.cvc_focused ? " focus": "")}>
                                {this.state.card_display_cvc}
                            </div>
                        </div>
                    </div>
                </div>
                <input className="card-name" type="text" ref="name" placeholder="Name on card" required
                       autoComplete="cc-name"
                       value={this.state.card_name} onChange={this.updateCardName}
                       onFocus={() => this.setState({name_focused: true})}
                       onBlur={() => this.setState({name_focused: false})}/>
                <input className="card-number" type="text" ref="number" placeholder="Card number" required
                       autoComplete="cc-number" inputMode="numeric"
                       pattern="[0-9]*" onChange={this.updateCardNumber} maxLength={16}
                       value={this.state.card_number} onFocus={() => this.setState({number_focused: true})}
                       onBlur={() => this.setState({number_focused: false})}/>
                <input className="card-expiry" type="text" ref="expiry" placeholder="MM / YY" required
                       pattern="[0-9/]*" autoComplete="cc-exp"
                       value={this.state.card_expiry} onChange={this.updateCardExpiry}
                       onFocus={() => this.setState({expiry_focused: true})}
                       onBlur={() => this.setState({expiry_focused: false})}/>
                <input className="card-cvc" type="text" ref="cvc" placeholder="CVC" maxLength={4} required
                       pattern="[0-9]*" autoComplete="cc-csc" inputMode="numeric"
                       value={this.state.card_cvc} onChange={this.updateCardCvc}
                       onFocus={() => this.setState({cvc_focused: true})}
                       onBlur={() => this.setState({cvc_focused: false})}/>
                <button type="submit">Submit</button>
            </form>
        </div>;
    }
}