'use strict';

const {
    graphqlExpress,
    graphiqlExpress
} = require('graphql-server-express');

const bodyParser = require('body-parser');

const querySchema = require('../graph');

const Connector = require('../graph/storage/connector');
const {Substances} = require('../graph/storage/models');

module.exports = function* ({app, log}) {
    const baseQuerySchema = querySchema({log});

    app.use('/graph', bodyParser.json(), (req, res, next) =>
        graphqlExpress({
            schema: baseQuerySchema.schema,
            rootValue: baseQuerySchema.root(req, res),
            context: {
                substances: new Substances({
                    connector: new Connector()
                })
            }
        })(req, res, next)
    );

    app.use('/graphiql', graphiqlExpress({
        endpointURL: '/graph',
        query: '',
    }));
};
