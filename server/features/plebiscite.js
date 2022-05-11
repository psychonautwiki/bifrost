'use strict';

const {MongoClient} = require('mongodb');

const MONGO_DB = process.env.MONGO_DB;
const MONGO_URL = process.env.MONGO_URL;
const MONGO_COLLECTION = process.env.MONGO_COLLECTION;

class Plebiscite {
    _db = null

    async _establishConnectionIfNeeded() {
        if (!this._db) {
            this._db =
                await MongoClient
                    .connect(MONGO_URL)
                    .then(db =>
                        db
                            .db(MONGO_DB)
                            .collection(MONGO_COLLECTION),
                    )
                    .catch(
                        err => {
                            console.error(err);

                            this._db = null;

                            return this._establishConnectionIfNeeded();
                        }
                    );
        }

        return this._db;
    }

    async find({substance, offset, limit}) {
        const collection = await this._establishConnectionIfNeeded();

        const query = {};

        if (!substance === true) {
            query['substanceInfo.substance'] = substance;
        }

        return collection.find(query)
            .sort({'meta.published': -1})
            .skip(offset)
            .limit(limit)
            .toArray();
    }
}

module.exports = new Plebiscite();
