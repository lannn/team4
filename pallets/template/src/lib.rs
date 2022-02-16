#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://docs.substrate.io/v3/runtime/frame>
pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;
	use sp_std::vec::Vec;
	use frame_support::{
		sp_runtime::traits::Hash,
		transactional,
		traits::{ Currency, tokens::ExistenceRequirement }
	};

	type AccountOf<T> = <T as frame_system::Config>::AccountId;
	type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	#[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo)]
	#[scale_info(skip_type_params(T))]
	pub struct Book<T: Config> {
		pub title: Vec<u8>,
		pub url: Vec<u8>,
		pub price: Option<BalanceOf<T>>,
		pub description: Vec<u8>,
		pub owner: AccountOf<T>,
	}

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		type Currency: Currency<Self::AccountId>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	#[pallet::getter(fn books)]
	pub(super) type Books<T: Config> = StorageMap<_, Twox64Concat, T::Hash, Book<T>>;

	#[pallet::storage]
	#[pallet::getter(fn books_owned)]
	pub(super) type BooksOwned<T: Config> = StorageMap<
		_,
		Twox64Concat,
		T::AccountId,
		Vec<T::Hash>,
		ValueQuery,
	>;

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		Created(T::AccountId, T::Hash),
		Transferred(T::AccountId, T::AccountId, T::Hash),
		Bought(T::AccountId, T::AccountId, T::Hash, BalanceOf<T>),
		PriceSet(T::AccountId, T::Hash, Option<BalanceOf<T>>),
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		BookNotExist,
		NotBookOwner,
		NotEnoughBalance,
		BookBidPriceTooLow,
	}

	// Dispatchable functions allows users to interact with the pallet and invoke state changes.
	// These functions materialize as "extrinsics", which are often compared to transactions.
	// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(100)]
		pub fn create_book(
			origin: OriginFor<T>, 
			title: Vec<u8>,
			url: Vec<u8>,
			description: Vec<u8>,
			price: BalanceOf<T>,
		) -> DispatchResult {
			let sender = ensure_signed(origin)?;

			let book = Book::<T> {
				title: title,
				url: url,
				description: description,
				price: Some(price),
				owner: sender.clone(),
			};

			let book_id = Self::mint(&sender, book)?;

			log::info!("A book is created with ID: {:?}.", book_id);

			Self::deposit_event(Event::Created(sender, book_id));

			Ok(())
		}

		#[pallet::weight(100)]
		pub fn set_price(origin: OriginFor<T>, book_id: T::Hash, new_price: Option<BalanceOf<T>>) -> DispatchResult {
			let sender = ensure_signed(origin)?;

			ensure!(Self::is_book_owner(&book_id, &sender)?, <Error<T>>::NotBookOwner);

			let mut book = Self::books(&book_id).ok_or(<Error<T>>::BookNotExist)?;

			book.price = new_price.clone();

			<Books<T>>::insert(&book_id, book);

			Self::deposit_event(Event::PriceSet(sender, book_id, new_price));

			Ok(())
		}

		#[pallet::weight(100)]
		pub fn transfer(origin: OriginFor<T>, to: T::AccountId, book_id: T::Hash) -> DispatchResult {
			let from = ensure_signed(origin)?;

			ensure!(Self::is_book_owner(&book_id, &from)?, <Error<T>>::NotBookOwner);

			Self::transfer_book_to(&book_id, &to)?;

			Self::deposit_event(Event::Transferred(from, to, book_id));

			Ok(())
		}

		#[transactional]
		#[pallet::weight(100)]
		pub fn buy_book(origin: OriginFor<T>, book_id: T::Hash, bid_price: BalanceOf<T>) -> DispatchResult {
			let buyer = ensure_signed(origin)?;
			let book = <Books<T>>::get(book_id).unwrap();
			let seller = book.owner.clone();

			if let Some(ask_price) = book.price {
				ensure!(ask_price <= bid_price, <Error<T>>::BookBidPriceTooLow)
			}

			ensure!(T::Currency::free_balance(&buyer) >= bid_price, <Error<T>>::NotEnoughBalance);

			T::Currency::transfer(&buyer, &seller, bid_price, ExistenceRequirement::KeepAlive)?;

			Self::transfer_book_to(&book_id, &buyer)?;

			Self::deposit_event(Event::Bought(buyer, seller, book_id, bid_price));

			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		pub fn mint(owner: &T::AccountId, book: Book<T>) -> Result<T::Hash, Error<T>> {
			let book_id = T::Hashing::hash_of(&book);

			<Books<T>>::insert(book_id, book);
			<BooksOwned<T>>::mutate(&owner, |book_vec| {
				book_vec.push(book_id)
			});

			Ok(book_id)
		}

		pub fn is_book_owner(book_id: &T::Hash, account: &T::AccountId) -> Result<bool, Error<T>> {
			match Self::books(book_id) {
				Some(book) => Ok(book.owner == *account),
				None => Err(<Error<T>>::BookNotExist)
			}
		}

		#[transactional]
		pub fn transfer_book_to(book_id: &T::Hash, to: &T::AccountId) -> Result<(), Error<T>> {
			let mut book = Self::books(&book_id).ok_or(<Error<T>>::BookNotExist)?;

			let prev_owner = book.owner.clone();

			// Remove book_id from BooksOwned of previous book owner
			<BooksOwned<T>>::try_mutate(&prev_owner, |book_vec| {
				if let Some(index) = book_vec.iter().position(|&id| id == *book_id) {
					book_vec.swap_remove(index);
					return Ok(());
				}
				Err(())
			}).map_err(|_| <Error<T>>::BookNotExist)?;

			book.owner = to.clone();

			<Books<T>>::insert(book_id, book);
			<BooksOwned<T>>::mutate(to, |book_vec| {
				book_vec.push(*book_id)
			});

			Ok(())
		}
	}
}
