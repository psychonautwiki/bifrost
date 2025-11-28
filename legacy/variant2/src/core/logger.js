'use strict';

const bunyan = require('bunyan');
const config = require('../config');

const logger = bunyan.createLogger({
    name: config.logging.name,
    level: config.logging.level,
    serializers: bunyan.stdSerializers
});

module.exports = logger;
