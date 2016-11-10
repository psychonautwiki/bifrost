'use strict';

const _ = require('lodash');

module.exports = log => {
    if (global.v8debug) {
        log.info('Detected debug mode. Enabling break on exception.');
        global.v8debug.Debug.setBreakOnException();
    }
};
