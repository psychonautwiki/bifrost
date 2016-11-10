'use strict';

const RootQuery = `
type Substance {
    name: String
    url: String

	effects: [Effect]
	experiences: [Experience]
}

type Effect {
    name: String
    url: String

	substances: [Substance]
	experiences: [Experience]
}

type Experience {
	substances: [Substance]
	effects: [Experience]
}

type Query {
    substances(
        # Name of the effect you want the substances of
        effect: String

        # Name of the substance you are looking for
    	query: String

        limit: Int=50
        Offset: Int=0
    ): [Substance]

    effects(
        # Name of the substance you want the effects of
    	substance: String

        # Name of the effect you are looking for
        query: String

        limit: Int=50
        Offset: Int=0
    ): [Effect]

    experiences(
    	effect: String,
    	substance: String
    ): [Experience]
}
`;

module.exports = () => [RootQuery];
