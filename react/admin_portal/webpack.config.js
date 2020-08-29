const merge = require('webpack-merge');
const { WebpackPluginServe: Serve } = require('webpack-plugin-serve');
const common = require('./webpack.common.js');

module.exports = merge(common, {
  mode: 'development',
  plugins: [new Serve({ static: ["../../templates/admin", "../../static"] })],
  watch: true
});