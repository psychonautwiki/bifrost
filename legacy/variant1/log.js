'use strict';

const bunyan = require('bunyan');

module.exports = bunyan.createLogger({
    name: 'bifrost',
    level: process.env.LOG_LEVEL || 'info'
});
