'use strict';

const log = require('./log');

log.info(require('./ac'));

require('./testbed-bootstrap')(log);

const async = require('bluebird').coroutine;

const express = require('express');
const app = express();

const graphRoutes = require('./services/graph');

async(function* () {
    yield* graphRoutes({
        app, log
    });

    const host = process.env.HOST || '0.0.0.0';
    const port = process.env.PORT || 3000;

    app.listen(port, host, () =>
        log.info({type: 'server'}, `Online: ${host} ${port}`)
    );
})().catch(err => log.fatal(err));
