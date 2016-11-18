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

    app.use('/graphiql', graphiqlExpress({
        endpointURL: '/graph',
        query:
`{
  substances {
    name

    effects {
      name
    }
  }
}`,
    }));

    app.use('/', bodyParser.json(), (req, res, next) =>
        graphqlExpress({
            schema: baseQuerySchema.schema,
            rootValue: baseQuerySchema.root(req, res),
            context: {
                substances: new Substances({
                    connector: new Connector({log}),
                    log
                })
            }
        })(req, res, next)
    );
};
