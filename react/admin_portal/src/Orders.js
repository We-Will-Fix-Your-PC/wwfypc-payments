import React, {Component} from "react";
import {API_ROOT} from "./admin";
import SVG from "react-inlinesvg";
import loader from "./loader.svg";
import {Link} from "react-router-dom";

export default class Orders extends Component {
    constructor(props, context) {
        super(props, context);

        this.updateOrders = this.updateOrders.bind(this);
        this.nextList = this.nextList.bind(this);
        this.prevList = this.prevList.bind(this);

        this.state = {
            loading: true,
            offset: 0,
            limit: 5,
            orders: []
        };
    }

    componentDidMount() {
        this.updateOrders();
    }

    nextList() {
        this.setState({
            offset: this.state.offset + this.state.limit
        }, this.updateOrders);

    }

    prevList() {
        let nextVal = this.state.offset - this.state.limit;
        if (nextVal < 0) {
            nextVal = 0;
        }
        this.setState({
            offset: nextVal
        }, this.updateOrders);
    }

    updateOrders() {
        this.setState({
            loading: true,
        })
        fetch(`${API_ROOT}payments/?offset=${this.state.offset}&limit=${this.state.limit}`, {
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
                    orders: resp
                })
            })
    }

    render() {
        return <React.Fragment>
            <h2>Orders</h2>
            {this.state.loading ? <div className="loading">
                <SVG src={loader} className="loader"/>
            </div> : <React.Fragment>
                <table className="orders">
                    <thead>
                        <tr>
                            <th>ID</th>
                            <th>Timestamp</th>
                            <th>Environment</th>
                            <th>State</th>
                            <th>Payment method</th>
                            <th>Customer ID</th>
                            <th/>
                        </tr>
                    </thead>
                    <tbody>
                        {this.state.orders.map(order => {
                            return <tr key={order.id}>
                                <td>{order.id}</td>
                                <td>{order.timestamp}</td>
                                <td>{order.environment}</td>
                                <td>{order.state}</td>
                                <td>{order.payment_method}</td>
                                <td>
                                    <a target="_blank"
                                       href={`https://account.cardifftec.uk/auth/admin/wwfypc/console/#/realms/wwfypc/users/${order.customer.id}`}>
                                        {order.customer.id}
                                    </a>
                                </td>
                                <td>
                                    <div className="buttons">
                                        <button><Link to={`/order/${order.id}/`}>View</Link></button>
                                    </div>
                                </td>
                            </tr>
                        })}
                    </tbody>
                </table>
                <div className="buttons sideways">
                    <button disabled={this.state.offset === 0} onClick={this.prevList}>Previous</button>
                    <button disabled={this.state.orders.length !== this.state.limit} onClick={this.nextList}>Next</button>
                </div>
            </React.Fragment>
                }
        </React.Fragment>
    }
}