const express = require('express');
let app = express();
let orders = [];

const OrderStatus = Object.freeze({
    Deferred: 0,
    Reviewing: 1,
    Cooking: 3,
    OutForDelivery: 4,
    Delivered: 5,
    Unknown: 6,
});

function imageForStatus(status) {
    let ret = "/webfile?name=order-tracker-"
    switch (status) {
        case OrderStatus.Deferred:
            ret += "deferred.png";
            break;
            case OrderStatus.Reviewing:
            ret += "reviewing.png";
            break;
        case OrderStatus.Cooking:
            ret += "cooking.png";
            break;
        case OrderStatus.OutForDelivery:
            ret += "driving.png";
            break;
        case OrderStatus.Delivered:
            ret += "delivered.png";
            break;
        case OrderStatus.Unknown:
            ret += "unknown.png";
            break;
    }
    return ret;
} 

class Order {
    constructor(id, time, status = OrderStatus.Unknown) {
        this.orderId = id;
        this.orderTrackerLink = "http://localhost:8888/order/" + id;
        this.timeOrdered = time;
        this.status = status
    }
    toJSON() {
        return {
            orderId: this.orderId,
            orderTrackerLink: this.orderTrackerLink,
            orderStatusImage: imageForStatus(this.status),
            timeOrdered: this.timeOrdered,
        }
    }
    nextStatus() {
        if (this.status === 6) {
            this.status = 0;
        }
        if (this.status !== 5) {
            this.status++;
        }
    }
}
let threshold = 10;
function shouldChange() {
    let rnd = Math.floor(Math.random() * 100);
    let ret = rnd < threshold;
    threshold + 10;
    return ret;
}

class OrderListResponse {
    constructor(orders) {
        this.meta = {
            code: 200,
            error: '',
            info: '',
        };
        this.response = orders;
    }
}


app.get('/order/:id', (req, res) => {
    console.log('get /order/' + req.params.id);
    let order = orders.find(o => o.orderId == req.params.id);
    if (!order) {
        console.error('unable to find order');
        res.sendStatus(500);
    }
    order.nextStatus();
    if (shouldChange() || order.status == OrderStatus.Unknown) {
    }
    res.send(html(order.status));
});

app.get('/', (req, res) => {
    console.log('get ', req.path);
    res.send(JSON.stringify(new OrderListResponse(orders)));
    for (order of orders) {
        if (shouldChange() || order.status == OrderStatus.Unknown) {
            order.nextStatus();
        }
    }
});

function html(status) {
    return `<html><head></head><body><div id="currentStep">${status}</div></body></html>`
}

app.listen(8888, err => {
    if (err) throw err;
    orders.push(new Order(1, "Tue 18 Sep 2018 12:00:00", OrderStatus.Delivered)),
    orders.push(new Order(2, "Tue 19 Sep 2018 15:10:00"))
    console.log('Listening on 8888');
});

