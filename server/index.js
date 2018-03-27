'use strict';

const {ApolloEngine} = require('apollo-engine');

const log = require('./log');

log.info(require('./util/ac'));

require('./testbed-bootstrap')(log);

const async = require('bluebird').coroutine;

const express = require('express');
const app = express();

const apollo_key = process.env.ENGINE_API_KEY;

const graphRoutes = require('./services/graph');

async(function* () {
    yield* graphRoutes({
        app, log
    });

    const host = process.env.HOST || '0.0.0.0';
    const port = process.env.PORT || 3000;

    if (apollo_key) {
        const engine = new ApolloEngine({
            apiKey: apollo_key,
        });
    
        engine.listen({
            host, port,
            graphqlPaths: ['/'],
            expressApp: app,
            launcherOptions: {
                startupTimeout: 3000,
            },
        }, () =>
            log.info({type: 'server'}, `Online: ${host} ${port}`)
        );

        return;
    }

    app.listen(port, host, () =>
        log.info({type: 'server'}, `Online: ${host} ${port}`)
    );
})().catch(err => log.fatal(err));
