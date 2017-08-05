'use strict';

const _ = require('lodash');

const opticsAgent = require('optics-agent');

const {
    graphqlExpress,
    graphiqlExpress
} = require('graphql-server-express');

const bodyParser = require('body-parser');

const featureContext = {};

const features = require('../util/features');

if (features.has('plebiscite')) {
    _.assign(featureContext, {
        plebiscite: require('../features/plebiscite')
    });
}

const querySchema = require('../graph');

const Connector = require('../graph/storage/connector');
const {Substances} = require('../graph/storage/models');

const SMWDataArbitrator = require('../graph/helpers/smwDataArbitrator');
const smwDataArbitrator = new SMWDataArbitrator();

const PWPropParser = require('../graph/helpers/pwPropParser');

const pwPropParser = new PWPropParser({
    smwDataArbitrator
});

module.exports = function* ({app, log}) {
    const baseQuerySchema = querySchema({log});

    app.use(opticsAgent.middleware());

    app.get('/', graphiqlExpress({
        endpointURL: '/',
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

    app.post('/', bodyParser.json(), (req, res, next) =>
        graphqlExpress({
            schema: baseQuerySchema.schema,
            rootValue: baseQuerySchema.root(req, res),
            context: _.assign({}, {
                substances: new Substances({
                    connector: new Connector({log}),
                    pwPropParser,
                    log
                }),
                opticsContext: opticsAgent.context(req)
            }, featureContext)
        })(req, res, next)
    );
};
