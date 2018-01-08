'use strict';

const {Engine} = require('apollo-engine');

const log = require('./log');

log.info(require('./util/ac'));

require('./testbed-bootstrap')(log);

const async = require('bluebird').coroutine;

const express = require('express');
const app = express();

const apollo_key = process.env.ENGINE_API_KEY;

if (apollo_key) {
    const engine = new Engine({
        engineConfig: {
            apiKey: apollo_key
        },

        graphqlPort: process.env.PORT || 3000,
        endpoint: '/'
    });

    engine.start();

    app.use(engine.expressMiddleware());
}

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
