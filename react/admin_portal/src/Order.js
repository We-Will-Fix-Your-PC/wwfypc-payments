import React, {Component} from "react";
import {API_ROOT} from "./admin";
import SVG from "react-inlinesvg";
import loader from "./loader.svg";

export default class Order extends Component {
    constructor(props, context) {
        super(props, context);
        console.log(props);

        this.updateOrder = this.updateOrder.bind(this);

        this.state = {
            loading: true,
            order: null,
        };
    }

    componentDidMount() {
        this.updateOrder();
    }

    updateOrder() {
        this.setState({
            loading: true,
        })
        fetch(`${API_ROOT}payment/${this.props.match.params.id}/`, {
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
                    order: resp
                })
            })
    }

    render() {
        return <React.Fragment>
            <h2>Order</h2>
            {this.state.loading ? <div className="loading">
                <SVG src={loader} className="loader"/>
            </div> : <React.Fragment>
                <p style={{paddingLeft: 10, paddingRight: 10}}>
                    <b>ID:</b> {this.state.order.id}<br/>
                    <b>Timestamp:</b> {this.state.order.timestamp}<br/>
                    <b>State:</b> {this.state.order.state}<br/>
                    <b>Environment:</b> {this.state.order.environment}<br/>
                    <b>Customer:</b>
                    <a target="_blank"
                       href={`https://account.cardifftec.uk/auth/admin/wwfypc/console/#/realms/wwfypc/users/${this.state.order.customer.id}`}>
                        {this.state.order.customer.id}
                    </a><br/>
                    <b>Payment method:</b> {this.state.order.payment_method}<br/>
                </p>
                <h2>Items</h2>
                <table>
                    <thead>
                        <tr>
                            <th>ID</th>
                            <th>Type</th>
                            <th>Title</th>
                            <th>Price</th>
                            <th>Quantity</th>
                            <th>Data</th>
                        </tr>
                    </thead>
                    <tbody>
                    {this.state.order.items.map(item => <tr>
                        <td>{item.id}</td>
                        <td>{item.type}</td>
                        <td>{item.title}</td>
                        <td>{item.price}</td>
                        <td>{item.quantity}</td>
                        <td>{item.data}</td>
                    </tr>)}
                    </tbody>
                </table>
            </React.Fragment>
                }
        </React.Fragment>
    }
}