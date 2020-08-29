'use strict';

import React, {Component} from 'react';
import ReactDOM from 'react-dom';
import 'whatwg-fetch';
import SVG from "react-inlinesvg";
import loader from "./loader.svg";
import {
  BrowserRouter as Router,
  Switch,
  Route,
  Link
} from "react-router-dom";
import Orders from "./Orders";
import Order from "./Order";

export const API_ROOT = process.env.BASE_URL ? process.env.BASE_URL :
    process.env.NODE_ENV  === 'production' ? 'https://payments.cardifftec.uk/' : 'https://wwfypc-payments.eu.ngrok.io/';

class Admin extends Component {
    constructor(props, context) {
        super(props, context);

        this.handleError = this.handleError.bind(this);
        this.updateLoginState = this.updateLoginState.bind(this);
        this.openLoginPopup = this.openLoginPopup.bind(this);
        this.openLogoutPopup = this.openLogoutPopup.bind(this);
        this.handleMessage = this.handleMessage.bind(this);

        this.state = {
            loading: true,
            popupWindow: null,
            user_id: null
        }
    }

    handleError(err, message) {
        if (err) {
            console.error(err);
        }
        let error_msg = (message === undefined) ? "Something went wrong" : message;
        this.setState({
            err: error_msg,
            loading: false,
        })
    }

    openLoginPopup() {
        const win = window.open(
            `${API_ROOT}login/auth/?next=${API_ROOT}payment/login-complete/`,
            'login',
            'status=no,location=no,toolbar=no,menubar=no'
        );

        this.setState({
            popupWindow: win
        });
    }

    openLogoutPopup() {
        const win = window.open(
            `${API_ROOT}login/logout/?next=/payment/login-complete/`,
            'login',
            'status=no,location=no,toolbar=no,menubar=no'
        );

        this.setState({
            popupWindow: win
        });
    }

    handleMessage(event) {
        if (event.data.type === "login") {
            if (this.state.popupWindow) {
                this.state.popupWindow.close();
            }
            if (this.state.user_id === null) {
                if (!event.data.login_successful) {
                    this.handleError(null, "Login failed");
                } else {
                    this.updateLoginState();
                }
            } else {
                this.updateLoginState();
            }
        }
    }

    updateLoginState() {
        this.setState({
            loading: true,
        })
        fetch(`${API_ROOT}login/whoami/`, {
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
                    this.setState({
                        loading: false,
                        user_id: resp.user
                    })
                })
                .catch(err => this.handleError(err))
    }

    componentDidMount() {
        this.updateLoginState();
        window.addEventListener("message", this.handleMessage, false);
    }

    componentDidCatch(error, _info) {
        this.handleError(error);
    }

    render() {
        return <Router basename="/admin">
            <div className="payment admin" style={{minHeight: "100vh"}}>
                {this.state.err != null ? <React.Fragment>
                    <h3>{this.state.err}</h3>
                    <div className="buttons">
                        <button onClick={window.location.reload}>Reload</button>
                    </div>
                </React.Fragment> : this.state.user_id == null ? <React.Fragment>
                    <h1>Cardifftec Payments</h1>
                    {this.state.loading ? <SVG src={loader} className="loader"/> :
                         <div className="buttons">
                            <button onClick={this.openLoginPopup}>Login</button>
                        </div>
                    }
                </React.Fragment> : <React.Fragment>
                    <header>
                        <h2>Cardifftec Payments</h2>
                        <div className="buttons">
                            <button onClick={this.openLogoutPopup}>Logout</button>
                        </div>
                    </header>
                    <main>
                        <Switch>
                          <Route path="/order/:id/" render={(props) => <Order {...props}/> }/>
                          <Route path="/">
                            <Orders/>
                          </Route>
                        </Switch>
                    </main>
                </React.Fragment>}
            </div>
        </Router>
    }
}

ReactDOM.render(<Admin />, document.getElementById("root"));