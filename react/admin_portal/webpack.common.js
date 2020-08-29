const path = require("path");
const webpack = require('webpack');

module.exports = {
    entry: {
        admin: ['whatwg-fetch', "./src/admin.js", 'webpack-plugin-serve/client']
    },
    module: {
        rules: [
            {
                test: /\.(js|jsx)$/,
                exclude: /(node_modules|bower_components)/,
                loader: "babel-loader",
                options: {presets: ["@babel/env"]}
            },
            {
                test: /\.css$/,
                use: ["style-loader", "css-loader"]
            },
            {
                test: /\.svg$/,
                loader: 'svg-inline-loader'
            }
        ]
    },
    resolve: {extensions: ["*", ".js", ".jsx"]},
    output: {
        path: path.resolve(__dirname, "../../static/js/"),
        publicPath: "/static/js/",
        filename: "[name].js"
    },
    plugins: [
        new webpack.EnvironmentPlugin( { ...process.env } )
    ]
};